//! Decouverte des recepteurs DirTransfert sur le reseau local (broadcast UDP).
//!
//! La sonde ne contient que le tag de decouverte (HMAC protege par Argon2id) :
//! un attaquant du LAN ne peut ni usurper un recepteur ni deviner le code
//! rapidement hors ligne.

use crate::crypto;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

pub const DISCOVERY_PORT: u16 = 47501;
const MAGIC: &[u8; 5] = b"DTXD1";

pub struct Responder {
    sock: UdpSocket,
}

/// Ecoute les sondes de decouverte (a appeler pendant l'attente de connexion).
pub fn responder() -> Option<Responder> {
    let sock = UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)).ok()?;
    sock.set_nonblocking(true).ok()?;
    Some(Responder { sock })
}

impl Responder {
    /// Traite au plus une sonde en attente. Retourne un message de journal si
    /// une sonde valide a ete recue.
    pub fn poll(&self, code: &str, tcp_port: u16) -> Option<String> {
        let mut buf = [0u8; 64];
        let (n, from) = match self.sock.recv_from(&mut buf) {
            Ok(v) => v,
            Err(_) => return None,
        };
        if n < 5 + 1 + 16 || &buf[..5] != MAGIC || buf[5] != b'Q' {
            return None;
        }
        let tag = crypto::discovery_tag(code);
        if !crypto::ct_eq(&buf[6..22], &tag) {
            return None;
        }
        let mut resp = Vec::with_capacity(8);
        resp.extend_from_slice(MAGIC);
        resp.push(b'R');
        resp.extend_from_slice(&tcp_port.to_be_bytes());
        let _ = self.sock.send_to(&resp, from);
        Some(format!(
            "sonde de decouverte recue de {} (code verifie, reponse : port {})",
            from, tcp_port
        ))
    }
}

/// Envoie une sonde broadcast et collecte les recepteurs qui repondent.
pub fn discover(code: &str, timeout: Duration) -> Result<Vec<(IpAddr, u16)>, String> {
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("bind UDP : {}", e))?;
    sock.set_broadcast(true).ok();
    sock.set_read_timeout(Some(Duration::from_millis(200))).ok();
    let mut probe = Vec::with_capacity(22);
    probe.extend_from_slice(MAGIC);
    probe.push(b'Q');
    probe.extend_from_slice(&crypto::discovery_tag(code));
    let broadcast = SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), DISCOVERY_PORT);
    sock.send_to(&probe, broadcast)
        .map_err(|e| format!("envoi de la sonde : {}", e))?;
    if let Some(local) = crate::net::local_ipv4() {
        let o = local.octets();
        let directed = SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(o[0], o[1], o[2], 255)),
            DISCOVERY_PORT,
        );
        let _ = sock.send_to(&probe, directed);
    }
    let mut peers: Vec<(IpAddr, u16)> = Vec::new();
    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; 64];
    while Instant::now() < deadline {
        if let Ok((n, from)) = sock.recv_from(&mut buf) {
            if n >= 8 && &buf[..5] == MAGIC && buf[5] == b'R' {
                let port = u16::from_be_bytes([buf[6], buf[7]]);
                if !peers.iter().any(|(ip, _)| *ip == from.ip()) {
                    peers.push((from.ip(), port));
                }
            }
        }
    }
    if peers.is_empty() {
        Err("aucun recepteur DirTransfert trouve sur le reseau local (lancez receive sur l'autre PC)".into())
    } else {
        Ok(peers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Le port UDP de decouverte est unique : on serialise ces deux tests.
    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn sonde_et_reponse_aller_retour() {
        let _g = LOCK.lock().unwrap();
        let resp = responder().expect("bind UDP discovery");
        let code = "AB2CD-34567";
        // Sonde valide depuis un autre socket.
        let probe_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut probe = Vec::new();
        probe.extend_from_slice(MAGIC);
        probe.push(b'Q');
        probe.extend_from_slice(&crypto::discovery_tag(code));
        probe_sock
            .send_to(&probe, ("127.0.0.1", DISCOVERY_PORT))
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(resp.poll(code, 47500).is_some());
        // La reponse doit arriver sur le socket de la sonde.
        probe_sock
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut buf = [0u8; 64];
        let (n, _) = probe_sock.recv_from(&mut buf).unwrap();
        assert!(n >= 8 && &buf[..5] == MAGIC && buf[5] == b'R');
        assert_eq!(u16::from_be_bytes([buf[6], buf[7]]), 47500);
    }

    #[test]
    fn sonde_mauvais_code_ignoree() {
        let _g = LOCK.lock().unwrap();
        let resp = responder().expect("bind UDP discovery");
        let probe_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut probe = Vec::new();
        probe.extend_from_slice(MAGIC);
        probe.push(b'Q');
        probe.extend_from_slice(&crypto::discovery_tag("AUTRE-CODE1"));
        probe_sock
            .send_to(&probe, ("127.0.0.1", DISCOVERY_PORT))
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(resp.poll("AB2CD-34567", 47500).is_none());
    }

    #[test]
    fn tag_depend_du_code() {
        assert_ne!(
            crypto::discovery_tag("AB2CD-34567"),
            crypto::discovery_tag("AB2CD-34568")
        );
    }
}

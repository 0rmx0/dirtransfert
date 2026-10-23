use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::time::Duration;

const HTTP_TIMEOUT: Duration = Duration::from_secs(4);
const STUN_TIMEOUT: Duration = Duration::from_secs(3);
const NATPMP_TIMEOUT: Duration = Duration::from_millis(1500);
const STUN_MAGIC: u32 = 0x2112_A442;

const IP_HTTP_SOURCES: &[&str] = &[
    "http://api.ipify.org/",
    "http://ifconfig.me/",
    "http://checkip.amazonaws.com/",
    "http://icanhazip.com/",
];

const STUN_SERVERS: &[(&str, u16)] = &[("stun.l.google.com", 19302), ("stun.cloudflare.com", 3478)];

fn parse_ipv4(s: &str) -> Option<Ipv4Addr> {
    s.trim().parse::<Ipv4Addr>().ok()
}

fn http_get_body(url: &str) -> Result<String, String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("URL non supportee : {}", url))?;
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().unwrap_or(80)),
        None => (hostport, 80),
    };
    if host
        .parse::<IpAddr>()
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        .is_ipv6()
    {
        return Err("hote IPv6 non supporte ici".into());
    }
    let mut s =
        TcpStream::connect((host, port)).map_err(|e| format!("connexion a {} : {}", url, e))?;
    s.set_read_timeout(Some(HTTP_TIMEOUT)).ok();
    s.set_write_timeout(Some(HTTP_TIMEOUT)).ok();
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: DirTransfert\r\nConnection: close\r\n\r\n",
        path, hostport
    );
    s.write_all(req.as_bytes())
        .map_err(|e| format!("envoi HTTP : {}", e))?;
    let mut out = String::new();
    s.read_to_string(&mut out)
        .map_err(|e| format!("lecture reponse HTTP : {}", e))?;
    let body = match out.find("\r\n\r\n") {
        Some(i) => out[i + 4..].to_string(),
        None => String::new(),
    };
    Ok(body)
}

fn public_ip_http() -> Result<Ipv4Addr, String> {
    let mut last = String::new();
    for url in IP_HTTP_SOURCES {
        match http_get_body(url) {
            Ok(body) => {
                if let Some(ip) = parse_ipv4(&body) {
                    return Ok(ip);
                }
                last = format!("reponse invalide de {}", url);
            }
            Err(e) => last = e,
        }
    }
    Err(format!("aucun service HTTP n'a repondu ({})", last))
}

fn addr_from_stun_attr(family: u8, port: u16, ip4: [u8; 4], xored: bool) -> Option<SocketAddr> {
    if family != 0x01 {
        return None;
    }
    let ip = if xored {
        let m = STUN_MAGIC.to_be_bytes();
        [ip4[0] ^ m[0], ip4[1] ^ m[1], ip4[2] ^ m[2], ip4[3] ^ m[3]]
    } else {
        ip4
    };
    let port = if xored {
        port ^ (STUN_MAGIC >> 16) as u16
    } else {
        port
    };
    Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::from(ip)), port))
}

fn decode_stun_response(resp: &[u8]) -> Option<SocketAddr> {
    if resp.len() < 20 || &resp[0..2] != b"\x01\x01" {
        return None;
    }
    let msg_len = u16::from_be_bytes([resp[2], resp[3]]) as usize;
    if resp.len() < 20 + msg_len {
        return None;
    }
    let mut off = 20;
    let end = 20 + msg_len;
    while off + 4 <= end {
        let attr_type = u16::from_be_bytes([resp[off], resp[off + 1]]);
        let attr_len = u16::from_be_bytes([resp[off + 2], resp[off + 3]]) as usize;
        let v = off + 4;
        if v + attr_len > resp.len() {
            return None;
        }
        let (xored, wanted) = match attr_type {
            0x0020 => (true, true),
            0x0001 => (false, true),
            _ => (false, false),
        };
        if wanted && attr_len >= 8 {
            let family = resp[v + 1];
            let port = u16::from_be_bytes([resp[v + 2], resp[v + 3]]);
            let ip4 = [resp[v + 4], resp[v + 5], resp[v + 6], resp[v + 7]];
            if let Some(a) = addr_from_stun_attr(family, port, ip4, xored) {
                return Some(a);
            }
        }
        off = v + ((attr_len + 3) & !3);
    }
    None
}

fn public_ip_stun() -> Result<Ipv4Addr, String> {
    let mut last = String::new();
    for (host, port) in STUN_SERVERS {
        let addr = match std::net::ToSocketAddrs::to_socket_addrs(&(*host, *port)) {
            Ok(mut it) => it.find(|a| a.is_ipv4()),
            Err(e) => {
                last = format!("resolution de {} : {}", host, e);
                continue;
            }
        };
        let Some(server) = addr else {
            last = format!("{} : pas d'adresse IPv4", host);
            continue;
        };
        let sock = match UdpSocket::bind("0.0.0.0:0") {
            Ok(s) => s,
            Err(e) => {
                last = format!("bind UDP : {}", e);
                continue;
            }
        };
        sock.set_read_timeout(Some(STUN_TIMEOUT)).ok();
        let mut req = vec![0x00, 0x01, 0x00, 0x00];
        req.extend_from_slice(&STUN_MAGIC.to_be_bytes());
        let mut tx = [0u8; 12];
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        tx[..8].copy_from_slice(&(nanos as u64).to_be_bytes());
        tx[8..].copy_from_slice(&nanos.to_be_bytes()[..4]);
        req.extend_from_slice(&tx);
        if sock.send_to(&req, server).is_err() {
            last = format!("envoi STUN vers {}", server);
            continue;
        }
        let mut buf = [0u8; 548];
        match sock.recv_from(&mut buf) {
            Ok((n, _)) => {
                if let Some(a) = decode_stun_response(&buf[..n]) {
                    if let IpAddr::V4(ip) = a.ip() {
                        return Ok(ip);
                    }
                }
                last = format!("reponse STUN invalide de {}", server);
            }
            Err(e) => last = format!("reception STUN de {} : {}", server, e),
        }
    }
    Err(format!("aucun serveur STUN n'a repondu ({})", last))
}

pub fn is_cgnat(ip: Ipv4Addr) -> bool {
    ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1])
}

pub fn is_private_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 10
        || (o[0] == 172 && (16..=31).contains(&o[1]))
        || (o[0] == 192 && o[1] == 168)
        || o[0] == 127
        || (o[0] == 169 && o[1] == 254)
        || o[0] == 0
}

pub fn local_ipv4() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:80").ok()?;
    match s.local_addr().ok()?.ip() {
        IpAddr::V4(ip) => Some(ip),
        _ => None,
    }
}

pub fn global_ipv6() -> Option<IpAddr> {
    let s = UdpSocket::bind("[::]:0").ok()?;
    s.connect("[2001:4860:4860::8888]:53").ok()?;
    let ip = s.local_addr().ok()?.ip();
    match ip {
        IpAddr::V6(v6) => {
            if (v6.segments()[0] & 0xfe00) != 0xfc00 && !ip.is_loopback() && v6.segments()[0] != 0 {
                Some(ip)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetMode {
    Direct,
    NatOuvert,
    Cgnat,
}

impl NetMode {
    pub fn label(self) -> &'static str {
        match self {
            NetMode::Direct => "acces direct (IP publique sur le PC)",
            NetMode::NatOuvert => "derriere une box (NAT) : redirection de port possible",
            NetMode::Cgnat => "derriere un CGNAT operateur : la redirection de port est inoperante",
        }
    }
}

pub fn detect_net_mode() -> Result<(NetMode, Ipv4Addr), String> {
    let pub_ip = public_ip()?;
    let Some(local) = local_ipv4() else {
        return Ok((NetMode::Direct, pub_ip));
    };
    if !is_private_v4(local) && local == pub_ip {
        return Ok((NetMode::Direct, pub_ip));
    }
    if is_cgnat(pub_ip) {
        return Ok((NetMode::Cgnat, pub_ip));
    }
    Ok((NetMode::NatOuvert, pub_ip))
}

pub fn public_ip() -> Result<Ipv4Addr, String> {
    if let Ok(ip) = crate::upnp::get_external_ip() {
        if let Some(v4) = parse_ipv4(&ip) {
            return Ok(v4);
        }
    }
    if let Ok(ip) = public_ip_http() {
        return Ok(ip);
    }
    public_ip_stun()
}

fn gateway_candidates(local: Ipv4Addr) -> Vec<Ipv4Addr> {
    let o = local.octets();
    vec![
        Ipv4Addr::new(o[0], o[1], o[2], 1),
        Ipv4Addr::new(o[0], o[1], o[2], 254),
        Ipv4Addr::new(o[0], o[1], 0, 1),
    ]
}

fn natpmp_exchange(gw: Ipv4Addr, req: &[u8], expect_op: u8) -> Result<Vec<u8>, String> {
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("bind : {}", e))?;
    sock.set_read_timeout(Some(NATPMP_TIMEOUT)).ok();
    sock.send_to(req, SocketAddr::new(IpAddr::V4(gw), 5351))
        .map_err(|e| format!("envoi NAT-PMP a {} : {}", gw, e))?;
    let mut buf = [0u8; 32];
    let (n, _) = sock
        .recv_from(&mut buf)
        .map_err(|e| format!("pas de reponse NAT-PMP de {} : {}", gw, e))?;
    if n < 4 || buf[1] != expect_op || buf[0] != 0 {
        return Err("reponse NAT-PMP inattendue".into());
    }
    let result = u16::from_be_bytes([buf[2], buf[3]]);
    if result != 0 {
        return Err(format!("la box refuse NAT-PMP (code {})", result));
    }
    Ok(buf[..n].to_vec())
}

pub fn natpmp_add_mapping(port: u16) -> Result<Ipv4Addr, String> {
    let Some(local) = local_ipv4() else {
        return Err("IP locale introuvable".into());
    };
    let mut last = String::new();
    for gw in gateway_candidates(local) {
        let udp_probe = [
            0u8,
            1,
            0,
            port.to_be_bytes()[0],
            port.to_be_bytes()[1],
            0,
            0,
            0,
            0,
            60,
        ];
        if let Err(e) = natpmp_exchange(gw, &udp_probe, 129) {
            last = e;
            continue;
        }
        let tcp_req = [
            0u8,
            2,
            0,
            port.to_be_bytes()[0],
            port.to_be_bytes()[1],
            0,
            0,
            0,
            0,
            60,
        ];
        match natpmp_exchange(gw, &tcp_req, 130) {
            Ok(resp) if resp.len() >= 10 => {
                return Ok(gw);
            }
            Ok(_) => {
                last = "reponse NAT-PMP trop courte".into();
            }
            Err(e) => last = e,
        }
    }
    Err(format!("NAT-PMP impossible ({})", last))
}

pub fn natpmp_delete_mapping() -> Result<(), String> {
    let Some(local) = local_ipv4() else {
        return Err("IP locale introuvable".into());
    };
    for gw in gateway_candidates(local) {
        for op in [1u8, 2u8] {
            let req = [0u8, op, 0, 0, 0, 0, 0, 0, 0, 0];
            let _ = natpmp_exchange(gw, &req, 128 | op);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_xor_mapped_v4() {
        let mut resp = Vec::new();
        resp.extend_from_slice(&[0x01, 0x01, 0x00, 0x0c]);
        resp.extend_from_slice(&STUN_MAGIC.to_be_bytes());
        resp.extend_from_slice(&[0u8; 12]);
        let port: u16 = 47500;
        let xport = port ^ (STUN_MAGIC >> 16) as u16;
        let ip = Ipv4Addr::new(82, 65, 12, 34);
        let m = STUN_MAGIC.to_be_bytes();
        let xip = [
            ip.octets()[0] ^ m[0],
            ip.octets()[1] ^ m[1],
            ip.octets()[2] ^ m[2],
            ip.octets()[3] ^ m[3],
        ];
        resp.extend_from_slice(&0x0020u16.to_be_bytes());
        resp.extend_from_slice(&8u16.to_be_bytes());
        resp.push(0);
        resp.push(0x01);
        resp.extend_from_slice(&xport.to_be_bytes());
        resp.extend_from_slice(&xip);
        let a = decode_stun_response(&resp).expect("adresse decodable");
        assert_eq!(a.port(), 47500);
        assert_eq!(a.ip(), IpAddr::V4(ip));
    }

    #[test]
    fn decode_mapped_address_plain() {
        let mut resp = Vec::new();
        resp.extend_from_slice(&[0x01, 0x01, 0x00, 0x0c]);
        resp.extend_from_slice(&STUN_MAGIC.to_be_bytes());
        resp.extend_from_slice(&[7u8; 12]);
        resp.extend_from_slice(&0x0001u16.to_be_bytes());
        resp.extend_from_slice(&8u16.to_be_bytes());
        resp.push(0);
        resp.push(0x01);
        resp.extend_from_slice(&47500u16.to_be_bytes());
        resp.extend_from_slice(&[82, 65, 12, 34]);
        let a = decode_stun_response(&resp).expect("adresse decodable");
        assert_eq!(a.port(), 47500);
        assert_eq!(a.ip(), IpAddr::V4(Ipv4Addr::new(82, 65, 12, 34)));
    }

    #[test]
    fn decode_ignore_reponse_erreur() {
        assert!(decode_stun_response(&[0x01, 0x11, 0, 0, 0, 0]).is_none());
        assert!(decode_stun_response(&[]).is_none());
    }

    #[test]
    fn plages_cgnat_et_privees() {
        assert!(is_cgnat("100.64.1.2".parse().unwrap()));
        assert!(is_cgnat("100.127.0.1".parse().unwrap()));
        assert!(!is_cgnat("100.128.0.1".parse().unwrap()));
        assert!(!is_cgnat("82.65.12.34".parse().unwrap()));
        assert!(is_private_v4("192.168.1.5".parse().unwrap()));
        assert!(is_private_v4("10.0.0.1".parse().unwrap()));
        assert!(!is_private_v4("82.65.12.34".parse().unwrap()));
    }

    #[test]
    fn mode_reseau_selon_ips() {
        assert_eq!(
            detect_mode_from("192.168.1.5", "82.65.12.34"),
            NetMode::NatOuvert
        );
        assert_eq!(
            detect_mode_from("192.168.1.5", "100.64.1.2"),
            NetMode::Cgnat
        );
        assert_eq!(
            detect_mode_from("82.65.12.34", "82.65.12.34"),
            NetMode::Direct
        );
    }

    fn detect_mode_from(local: &str, public: &str) -> NetMode {
        let l: Ipv4Addr = local.parse().unwrap();
        let p: Ipv4Addr = public.parse().unwrap();
        if !is_private_v4(l) && l == p {
            NetMode::Direct
        } else if is_cgnat(p) {
            NetMode::Cgnat
        } else {
            NetMode::NatOuvert
        }
    }
}

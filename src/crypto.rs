use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Key, Nonce,
};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub const CODE_LEN: usize = 10;
const ARGON2_M_KIB: u32 = 65536;
const ARGON2_T: u32 = 2;
const ARGON2_P: u32 = 1;
const MAX_FRAME: usize = 512 * 1024 * 1024;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

type HmacSha256 = Hmac<Sha256>;

pub fn gen_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut raw = [0u8; CODE_LEN];
    rand::rng().fill_bytes(&mut raw);
    let s: Vec<u8> = raw
        .iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()])
        .collect();
    let s = String::from_utf8(s).unwrap();
    format!("{}-{}", &s[..5], &s[5..])
}

pub fn norm_code(input: &str) -> Option<String> {
    let cleaned: String = input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if cleaned.len() != CODE_LEN {
        return None;
    }
    if !cleaned
        .chars()
        .all(|c| "ABCDEFGHJKLMNPQRSTUVWXYZ23456789".contains(c))
    {
        return None;
    }
    Some(format!("{}-{}", &cleaned[..5], &cleaned[5..]))
}

fn derive_master(code: &str, salt: &[u8]) -> [u8; 32] {
    let params =
        argon2::Params::new(ARGON2_M_KIB, ARGON2_T, ARGON2_P, Some(32)).expect("parametres argon2");
    let a2 = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut out = [0u8; 32];
    a2.hash_password_into(code.as_bytes(), salt, &mut out)
        .expect("argon2");
    out
}

fn hmac_tag(master: &[u8; 32], msg: &[u8]) -> [u8; 32] {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(master).expect("cle hmac");
    mac.update(msg);
    mac.finalize().into_bytes().into()
}

/// Cle de session fraiche par connexion : depend du code ET des deux nonces
/// echanges pendant la poignee de main, donc jamais rejouable.
fn session_key(master: &[u8; 32], hello_c: &[u8; 32], hello_s: &[u8; 32]) -> [u8; 32] {
    let mut salt = Vec::with_capacity(64);
    salt.extend_from_slice(hello_c);
    salt.extend_from_slice(hello_s);
    let hk = Hkdf::<Sha256>::new(Some(&salt), master);
    let mut out = [0u8; 32];
    hk.expand(b"dtx2-session", &mut out).expect("hkdf");
    out
}

fn auth_token(master: &[u8; 32], hello_c: &[u8; 32], hello_s: &[u8; 32]) -> [u8; 32] {
    let mut m = Vec::with_capacity(69);
    m.extend_from_slice(b"dtx2-auth");
    m.extend_from_slice(hello_c);
    m.extend_from_slice(hello_s);
    hmac_tag(master, &m)
}

fn client_proof(
    master: &[u8; 32],
    hello_c: &[u8; 32],
    hello_s: &[u8; 32],
    token: &[u8; 32],
) -> [u8; 32] {
    let mut m = Vec::with_capacity(101);
    m.extend_from_slice(b"dtx2-proof");
    m.extend_from_slice(hello_c);
    m.extend_from_slice(hello_s);
    m.extend_from_slice(token);
    hmac_tag(master, &m)
}

/// Tag de découverte LAN : prouve la connaissance du code sans le reveler.
/// Protege par Argon2id, donc pas de dictionnaire offline rapide.
pub fn discovery_tag(code: &str) -> [u8; 16] {
    let master = derive_master(code, b"dtx2-discovery-salt-v1");
    let tag = hmac_tag(&master, b"dtx2-discover");
    let mut out = [0u8; 16];
    out.copy_from_slice(&tag[..16]);
    out
}

const DIR_CLIENT_TO_SERVER: u8 = 0;
const DIR_SERVER_TO_CLIENT: u8 = 1;

pub(crate) fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Compteur de trame avec l'octet de direction XoRe dans le premier octet,
/// pour eviter toute collision de nonce entre les deux sens du flux.
fn direction_frame(ctr: u64, dir: u8) -> [u8; 8] {
    let mut f = ctr.to_be_bytes();
    f[0] ^= dir;
    f
}

fn nonce_from(frame: [u8; 8]) -> Nonce {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&frame);
    Nonce::from(n)
}

pub struct SecureStream {
    inner: TcpStream,
    cipher: ChaCha20Poly1305,
    send_frame: u64,
    recv_frame: u64,
    send_dir: u8,
    recv_dir: u8,
    recv_buf: Vec<u8>,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
}

impl SecureStream {
    /// Poignee de main authentifiee mutuelle avec sel et challenge aleatoires :
    /// un rejeu d'une poignee de main capturee est rejete (la cle de session
    /// depend du challenge du serveur, frais a chaque connexion).
    pub fn server_handshake(mut inner: TcpStream, code: &str) -> Result<Self, String> {
        inner
            .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
            .map_err(|e| format!("timeout lecture : {}", e))?;
        inner
            .set_write_timeout(Some(HANDSHAKE_TIMEOUT))
            .map_err(|e| format!("timeout ecriture : {}", e))?;
        let mut hello_c = [0u8; 32];
        inner
            .read_exact(&mut hello_c)
            .map_err(|e| format!("lecture du hello client : {}", e))?;
        let mut salt = [0u8; 16];
        rand::rng().fill_bytes(&mut salt);
        let mut hello_s = [0u8; 32];
        rand::rng().fill_bytes(&mut hello_s);
        let mut hello = Vec::with_capacity(48);
        hello.extend_from_slice(&salt);
        hello.extend_from_slice(&hello_s);
        inner
            .write_all(&hello)
            .map_err(|e| format!("envoi du sel/challenge : {}", e))?;
        let master = derive_master(code, &salt);
        let token = auth_token(&master, &hello_c, &hello_s);
        inner
            .write_all(&token)
            .map_err(|e| format!("envoi du jeton : {}", e))?;
        let mut proof = [0u8; 32];
        inner
            .read_exact(&mut proof)
            .map_err(|e| format!("lecture de la preuve du client : {}", e))?;
        let expected = client_proof(&master, &hello_c, &hello_s, &token);
        if !ct_eq(&proof, &expected) {
            return Err("code de jumelage incorrect cote client".into());
        }
        let key = session_key(&master, &hello_c, &hello_s);
        Ok(Self::finish(
            inner,
            key,
            DIR_SERVER_TO_CLIENT,
            DIR_CLIENT_TO_SERVER,
        ))
    }

    pub fn client_handshake(mut inner: TcpStream, code: &str) -> Result<Self, String> {
        inner
            .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
            .map_err(|e| format!("timeout lecture : {}", e))?;
        inner
            .set_write_timeout(Some(HANDSHAKE_TIMEOUT))
            .map_err(|e| format!("timeout ecriture : {}", e))?;
        let mut hello_c = [0u8; 32];
        rand::rng().fill_bytes(&mut hello_c);
        inner
            .write_all(&hello_c)
            .map_err(|e| format!("envoi du hello : {}", e))?;
        let mut hello = [0u8; 48];
        inner
            .read_exact(&mut hello)
            .map_err(|e| format!("lecture du sel/challenge : {}", e))?;
        let salt = &hello[..16];
        let hello_s: [u8; 32] = hello[16..].try_into().unwrap();
        let master = derive_master(code, salt);
        let mut recv_token = [0u8; 32];
        inner
            .read_exact(&mut recv_token)
            .map_err(|e| format!("lecture du jeton : {}", e))?;
        if !ct_eq(&recv_token, &auth_token(&master, &hello_c, &hello_s)) {
            return Err("code de jumelage incorrect (poignee de main refusee)".into());
        }
        let token = recv_token;
        let proof = client_proof(&master, &hello_c, &hello_s, &token);
        inner
            .write_all(&proof)
            .map_err(|e| format!("envoi de la preuve : {}", e))?;
        let key = session_key(&master, &hello_c, &hello_s);
        Ok(Self::finish(
            inner,
            key,
            DIR_CLIENT_TO_SERVER,
            DIR_SERVER_TO_CLIENT,
        ))
    }

    fn finish(inner: TcpStream, key: [u8; 32], send_dir: u8, recv_dir: u8) -> Self {
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
        SecureStream {
            inner,
            cipher,
            send_frame: 0,
            recv_frame: 0,
            send_dir,
            recv_dir,
            recv_buf: Vec::new(),
            read_timeout: None,
            write_timeout: None,
        }
    }

    pub fn set_timeouts(&mut self, read: Option<Duration>, write: Option<Duration>) {
        self.inner.set_read_timeout(read).ok();
        self.inner.set_write_timeout(write).ok();
        self.read_timeout = read;
        self.write_timeout = write;
    }

    fn write_encrypted(&mut self, plaintext: &[u8]) -> std::io::Result<()> {
        let frame = direction_frame(self.send_frame, self.send_dir);
        self.send_frame = self.send_frame.wrapping_add(1);
        let aad = frame;
        let ct = self
            .cipher
            .encrypt(
                &nonce_from(frame),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| std::io::Error::other("chiffrement impossible"))?;
        let mut framed = Vec::with_capacity(4 + ct.len());
        framed.extend_from_slice(&(ct.len() as u32).to_be_bytes());
        framed.extend_from_slice(&ct);
        self.inner.write_all(&framed)
    }

    fn read_decrypted(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        let mut hdr = [0u8; 4];
        match self.inner.read_exact(&mut hdr) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }
        let len = u32::from_be_bytes(hdr) as usize;
        if len > MAX_FRAME + 16 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "trame trop volumineuse",
            ));
        }
        let mut ct = vec![0u8; len];
        self.inner.read_exact(&mut ct)?;
        let frame = direction_frame(self.recv_frame, self.recv_dir);
        self.recv_frame = self.recv_frame.wrapping_add(1);
        let aad = frame;
        let plaintext = self
            .cipher
            .decrypt(
                &nonce_from(frame),
                Payload {
                    msg: &ct,
                    aad: &aad,
                },
            )
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "authentification de la trame invalide",
                )
            })?;
        Ok(Some(plaintext))
    }
}

impl Write for SecureStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.write_encrypted(buf)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl Read for SecureStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        while self.recv_buf.is_empty() {
            match self.read_decrypted()? {
                Some(v) => self.recv_buf = v,
                None => return Ok(0),
            }
        }
        let n = buf.len().min(self.recv_buf.len());
        buf[..n].copy_from_slice(&self.recv_buf[..n]);
        self.recv_buf.drain(..n);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_CODE: &str = "AB2CD-34567";

    #[test]
    fn normalisation_code() {
        assert_eq!(norm_code("ab2cd-34567").unwrap(), "AB2CD-34567");
        assert_eq!(norm_code(" ab2cd34567 ").unwrap(), "AB2CD-34567");
        assert!(norm_code("ab2cd-3456").is_none());
        assert!(norm_code("ab2cd-345678").is_none());
        assert!(norm_code("AB2CD-3456O").is_none());
        assert!(norm_code("AB2CD-34560").is_none());
    }

    #[test]
    fn gen_code_valide() {
        let c = gen_code();
        assert_eq!(norm_code(&c).unwrap(), c);
    }

    #[test]
    fn derive_master_deterministe_et_distinct() {
        assert_eq!(
            derive_master(TEST_CODE, b"sel-fixe"),
            derive_master(TEST_CODE, b"sel-fixe")
        );
        assert_ne!(
            derive_master(TEST_CODE, b"sel-fixe"),
            derive_master(TEST_CODE, b"autre-sel")
        );
        assert_ne!(
            derive_master(TEST_CODE, b"sel-fixe"),
            derive_master("AB2CD-34568", b"sel-fixe")
        );
    }

    #[test]
    fn sens_distincts_nonces_distincts() {
        assert_ne!(
            direction_frame(0, DIR_CLIENT_TO_SERVER),
            direction_frame(0, DIR_SERVER_TO_CLIENT)
        );
        assert_ne!(
            direction_frame(5, DIR_CLIENT_TO_SERVER),
            direction_frame(5, DIR_SERVER_TO_CLIENT)
        );
        assert_ne!(
            nonce_from(direction_frame(0, DIR_CLIENT_TO_SERVER)),
            nonce_from(direction_frame(0, DIR_SERVER_TO_CLIENT))
        );
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
    }

    #[test]
    fn handshake_rejette_mauvais_code() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            SecureStream::server_handshake(s, TEST_CODE)
        });
        let c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        assert!(SecureStream::client_handshake(c, "WRONG-CODE").is_err());
        assert!(server.join().unwrap().is_err());
    }

    #[test]
    fn handshake_rejeu_rejete() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let mut results = Vec::new();
            for _ in 0..2 {
                let (s, _) = listener.accept().unwrap();
                results.push(SecureStream::server_handshake(s, TEST_CODE).is_ok());
            }
            results
        });
        // Client legit : poignee de main manuelle en enregistrant les octets emis.
        let c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut c = c;
        let mut hello_c = [0u8; 32];
        rand::rng().fill_bytes(&mut hello_c);
        c.write_all(&hello_c).unwrap();
        let mut hello = [0u8; 48];
        c.read_exact(&mut hello).unwrap();
        let salt = &hello[..16];
        let hello_s: [u8; 32] = hello[16..].try_into().unwrap();
        let master = derive_master(TEST_CODE, salt);
        let mut token = [0u8; 32];
        c.read_exact(&mut token).unwrap();
        assert!(ct_eq(&token, &auth_token(&master, &hello_c, &hello_s)));
        let proof = client_proof(&master, &hello_c, &hello_s, &token);
        c.write_all(&proof).unwrap();
        drop(c);
        // Attaquant : rejoue le hello et la preuve captures sur une nouvelle connexion.
        let a = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut a = a;
        a.write_all(&hello_c).unwrap();
        let mut hello2 = [0u8; 48];
        a.read_exact(&mut hello2).unwrap();
        assert_ne!(
            &hello2[16..],
            &hello_s,
            "le challenge serveur doit etre frais"
        );
        let mut token2 = [0u8; 32];
        a.read_exact(&mut token2).unwrap();
        a.write_all(&proof).unwrap();
        drop(a);
        let results = server.join().unwrap();
        assert_eq!(results, vec![true, false]);
    }

    #[test]
    fn roundtrip_chiffre_via_socket() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let mut ss = SecureStream::server_handshake(s, TEST_CODE).unwrap();
            let mut got = Vec::new();
            ss.read_to_end(&mut got).unwrap();
            got
        });
        let c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut cs = SecureStream::client_handshake(c, TEST_CODE).unwrap();
        cs.write_all(b"bonjour dirtransfert").unwrap();
        cs.flush().unwrap();
        drop(cs);
        assert_eq!(server.join().unwrap(), b"bonjour dirtransfert");
    }

    #[test]
    fn sessions_distinctes_cles_distinctes() {
        let master = derive_master(TEST_CODE, b"sel-de-test-16oct");
        let h1 = [1u8; 32];
        let h2 = [2u8; 32];
        assert_ne!(
            session_key(&master, &h1, &h2),
            session_key(&master, &h2, &h1)
        );
        assert_ne!(
            session_key(&master, &h1, &h2),
            session_key(&master, &h1, &h1)
        );
    }
}

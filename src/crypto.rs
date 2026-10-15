use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Key, Nonce,
};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub const CODE_LEN: usize = 8;
const ARGON2_M_KIB: u32 = 65536;
const ARGON2_T: u32 = 2;
const ARGON2_P: u32 = 1;
const MAX_FRAME: usize = 512 * 1024 * 1024;

pub fn gen_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut raw = [0u8; CODE_LEN];
    rand::rng().fill_bytes(&mut raw);
    let s: Vec<u8> = raw
        .iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()])
        .collect();
    let s = String::from_utf8(s).unwrap();
    format!("{}-{}", &s[..4], &s[4..])
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
    Some(format!("{}-{}", &cleaned[..4], &cleaned[4..]))
}

fn derive_key(code: &str) -> [u8; 32] {
    let salt = format!("dtx2:{}", code);
    let params =
        argon2::Params::new(ARGON2_M_KIB, ARGON2_T, ARGON2_P, Some(32)).expect("parametres argon2");
    let a2 = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut out = [0u8; 32];
    a2.hash_password_into(code.as_bytes(), salt.as_bytes(), &mut out)
        .expect("argon2");
    out
}

fn auth_token(key: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"dtx2-auth");
    h.update(key);
    h.finalize().into()
}

fn client_proof(hello: &[u8; 32], token: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"dtx2-proof");
    h.update(hello);
    h.update(token);
    h.finalize().into()
}

const DIR_CLIENT_TO_SERVER: u8 = 0;
const DIR_SERVER_TO_CLIENT: u8 = 1;

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
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
    pub fn server_handshake(mut inner: TcpStream, code: &str) -> Result<Self, String> {
        let key = derive_key(code);
        let token = auth_token(&key);
        let mut hello = [0u8; 32];
        inner
            .read_exact(&mut hello)
            .map_err(|e| format!("lecture de la poignee de main : {}", e))?;
        inner
            .write_all(&token)
            .map_err(|e| format!("envoi du jeton : {}", e))?;
        let mut proof = [0u8; 32];
        inner
            .read_exact(&mut proof)
            .map_err(|e| format!("lecture de la preuve du client : {}", e))?;
        let expected = client_proof(&hello, &token);
        if !ct_eq(&proof, &expected) {
            return Err("code de jumelage incorrect cote client".into());
        }
        Ok(Self::finish(
            inner,
            key,
            DIR_SERVER_TO_CLIENT,
            DIR_CLIENT_TO_SERVER,
        ))
    }

    pub fn client_handshake(mut inner: TcpStream, code: &str) -> Result<Self, String> {
        let key = derive_key(code);
        let token = auth_token(&key);
        let mut hello = [0u8; 32];
        rand::rng().fill_bytes(&mut hello);
        inner
            .write_all(&hello)
            .map_err(|e| format!("envoi du hello : {}", e))?;
        let mut recv_token = [0u8; 32];
        inner
            .read_exact(&mut recv_token)
            .map_err(|e| format!("lecture du jeton : {}", e))?;
        if !ct_eq(&recv_token, &token) {
            return Err("code de jumelage incorrect (poignee de main refusee)".into());
        }
        let proof = client_proof(&hello, &token);
        inner
            .write_all(&proof)
            .map_err(|e| format!("envoi de la preuve : {}", e))?;
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

    #[test]
    fn normalisation_code() {
        assert_eq!(norm_code("abcd-2345").unwrap(), "ABCD-2345");
        assert_eq!(norm_code(" abcd2345 ").unwrap(), "ABCD-2345");
        assert!(norm_code("abc-234").is_none());
        assert!(norm_code("ABCDEFGHI").is_none());
        assert!(norm_code("ABCD234O").is_none());
    }

    #[test]
    fn gen_code_valide() {
        let c = gen_code();
        assert_eq!(norm_code(&c).unwrap(), c);
    }

    #[test]
    fn derive_key_deterministe_et_distincte() {
        assert_eq!(derive_key("ABCD-2345"), derive_key("ABCD-2345"));
        assert_ne!(derive_key("ABCD-2345"), derive_key("ABCD-2346"));
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
            SecureStream::server_handshake(s, "ABCD-2345")
        });
        let c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        assert!(SecureStream::client_handshake(c, "WRONG-CODE").is_err());
        assert!(server.join().unwrap().is_err());
    }

    #[test]
    fn roundtrip_chiffre_via_socket() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let mut ss = SecureStream::server_handshake(s, "ABCD-2345").unwrap();
            let mut got = Vec::new();
            ss.read_to_end(&mut got).unwrap();
            got
        });
        let c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut cs = SecureStream::client_handshake(c, "ABCD-2345").unwrap();
        cs.write_all(b"bonjour dirtransfert").unwrap();
        cs.flush().unwrap();
        drop(cs);
        assert_eq!(server.join().unwrap(), b"bonjour dirtransfert");
    }
}

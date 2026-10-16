use sha2::{Digest, Sha256};

const LINK_PREFIX: &str = "DTX1";
const LINK_VERSION: u8 = 1;
const B32_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const MAX_HOST: usize = 255;

pub struct LinkInfo {
    pub host: String,
    pub port: u16,
    pub code: String,
}

fn b32_encode(data: &[u8]) -> String {
    let mut out = String::new();
    let mut buf: u64 = 0;
    let mut bits: u32 = 0;
    for &b in data {
        buf = (buf << 8) | b as u64;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(B32_ALPHABET[((buf >> bits) & 0x1F) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(B32_ALPHABET[((buf << (5 - bits)) & 0x1F) as usize] as char);
    }
    out
}

fn b32_decode(s: &str) -> Option<Vec<u8>> {
    let mut buf: u64 = 0;
    let mut bits: u32 = 0;
    let mut out = Vec::new();
    for c in s.chars() {
        let v = B32_ALPHABET.iter().position(|&a| a as char == c)? as u64;
        if bits + 5 > 64 {
            return None;
        }
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    if bits > 0 && (buf & ((1 << bits) - 1)) != 0 {
        return None;
    }
    Some(out)
}

fn keystream(len: usize, seed: u8) -> Vec<u8> {
    let mut ks = Vec::with_capacity(len);
    let mut ctr: u32 = 0;
    while ks.len() < len {
        let mut h = Sha256::new();
        h.update(b"dtx1-link-keystream");
        h.update(seed.to_be_bytes());
        h.update(ctr.to_be_bytes());
        ks.extend_from_slice(&h.finalize());
        ctr = ctr.wrapping_add(1);
    }
    ks.truncate(len);
    ks
}

fn checksum(payload: &[u8]) -> [u8; 2] {
    let mut h = Sha256::new();
    h.update(b"dtx1-link-checksum");
    h.update(payload);
    let d = h.finalize();
    [d[0], d[1]]
}

fn code_bytes(code: &str) -> Option<[u8; 8]> {
    let cleaned: String = code
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if cleaned.len() != 8 {
        return None;
    }
    Some(cleaned.as_bytes().try_into().unwrap())
}

pub fn encode(host: &str, port: u16, code: &str) -> Result<String, String> {
    let host = host.trim();
    if host.is_empty() || host.len() > MAX_HOST {
        return Err("hote invalide pour le code de connexion".into());
    }
    let code = crate::crypto::norm_code(code).ok_or("code de jumelage invalide")?;
    let cb = code_bytes(&code).unwrap();
    let mut payload = Vec::with_capacity(2 + host.len() + 2 + 8);
    payload.push(LINK_VERSION);
    payload.push(host.len() as u8);
    payload.extend_from_slice(host.as_bytes());
    payload.extend_from_slice(&port.to_be_bytes());
    payload.extend_from_slice(&cb);
    let cs = checksum(&payload);
    payload.extend_from_slice(&cs);
    let ks = keystream(payload.len(), payload.len() as u8);
    let mixed: Vec<u8> = payload.iter().zip(ks.iter()).map(|(a, b)| a ^ b).collect();
    let b32 = b32_encode(&mixed);
    let mut out = String::from(LINK_PREFIX);
    let chars: Vec<char> = b32.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if i % 5 == 0 {
            out.push('-');
        }
        out.push(*c);
    }
    Ok(out)
}

pub fn decode(input: &str) -> Result<LinkInfo, String> {
    let cleaned: String = input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let rest = cleaned
        .strip_prefix(LINK_PREFIX)
        .ok_or("code de connexion invalide (prefixe DTX1 absent)")?;
    if rest.is_empty() {
        return Err("code de connexion vide apres DTX1".into());
    }
    let mixed =
        b32_decode(rest).ok_or("code de connexion illisible (caracteres base32 invalides)")?;
    let ks = keystream(mixed.len(), mixed.len() as u8);
    let payload: Vec<u8> = mixed.iter().zip(ks.iter()).map(|(a, b)| a ^ b).collect();
    if payload.len() < 2 {
        return Err("code de connexion tronque".into());
    }
    let (body, cs) = payload.split_at(payload.len() - 2);
    if checksum(body) != cs {
        return Err("code de connexion invalide (checksum incorrect, verifiez la saisie)".into());
    }
    if body.first() != Some(&LINK_VERSION) {
        return Err("version du code de connexion non prise en charge".into());
    }
    if body.len() < 2 {
        return Err("code de connexion tronque".into());
    }
    let hlen = body[1] as usize;
    if body.len() < 2 + hlen + 10 {
        return Err("code de connexion tronque (hote)".into());
    }
    let host = String::from_utf8(body[2..2 + hlen].to_vec())
        .map_err(|_| "hote non-UTF8 dans le code de connexion".to_string())?;
    let off = 2 + hlen;
    let port = u16::from_be_bytes([body[off], body[off + 1]]);
    let cb: [u8; 8] = body[off + 2..off + 10]
        .try_into()
        .map_err(|_| "code de jumelage tronque".to_string())?;
    let code_str = String::from_utf8(cb.to_vec())
        .map_err(|_| "code de jumelage illisible".to_string())?;
    let code = crate::crypto::norm_code(&code_str).ok_or("code de jumelage invalide")?;
    Ok(LinkInfo { host, port, code })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_ipv4() {
        let l = encode("192.168.1.42", 47500, "ABCD-2345").unwrap();
        assert!(l.starts_with("DTX1-"));
        let i = decode(&l).unwrap();
        assert_eq!(i.host, "192.168.1.42");
        assert_eq!(i.port, 47500);
        assert_eq!(i.code, "ABCD-2345");
    }

    #[test]
    fn roundtrip_ipv6_et_hostname() {
        for host in ["fe80::a1b2", "ma-box.example.net"] {
            let l = encode(host, 1234, "ABCD-6789").unwrap();
            let i = decode(&l).unwrap();
            assert_eq!(i.host, host);
            assert_eq!(i.port, 1234);
            assert_eq!(i.code, "ABCD-6789");
        }
    }

    #[test]
    fn decode_tolerant_casse_et_separateurs() {
        let l = encode("10.0.0.5", 47500, "ABCD-2345").unwrap();
        let mut mangled = String::new();
        for (i, c) in l.chars().enumerate() {
            if c == '-' {
                continue;
            }
            if i % 3 == 0 {
                mangled.push(c.to_ascii_lowercase());
            } else {
                mangled.push(c);
            }
        }
        let i = decode(&mangled).unwrap();
        assert_eq!(i.host, "10.0.0.5");
        assert_eq!(i.code, "ABCD-2345");
    }

    #[test]
    fn corruption_rejetee() {
        let l = encode("192.168.1.42", 47500, "ABCD-2345").unwrap();
        let cleaned: String = l.chars().filter(|c| *c != '-').collect();
        let mut corrupted = String::new();
        for (i, c) in cleaned.chars().enumerate() {
            if i == 8 {
                corrupted.push(if c == 'A' { 'B' } else { 'A' });
            } else {
                corrupted.push(c);
            }
        }
        assert!(decode(&corrupted).is_err());
        assert!(decode("DTX1-AAAA").is_err());
        assert!(decode("ABCD-2345").is_err());
    }

    #[test]
    fn b32_canonique() {
        assert_eq!(b32_decode(&b32_encode(&[0u8; 18])).unwrap(), vec![0u8; 18]);
        assert!(b32_decode("ABBB").is_none());
        assert_eq!(b32_encode(b"foobar"), "MZXW6YTBOI");
    }
}

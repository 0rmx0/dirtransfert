use sha2::Digest;
use std::io::{Read, Write};
use std::path::PathBuf;

pub const MAGIC: [u8; 4] = *b"DTX2";
pub const CHUNK_SIZE: usize = 1024 * 1024;
#[allow(dead_code)]
pub const DEFAULT_PORT: u16 = 47500;

/// Manifeste avec bit0 = morceaux compresses (zstd, par morceau de CHUNK_SIZE).
pub const FLAG_COMPRESSED: u8 = 1;

#[derive(Debug, Clone, PartialEq)]
pub struct FileEntry {
    pub rel_path: String,
    pub size: u64,
    pub hash: [u8; 32],
    /// Hash SHA-256 de chaque morceau de CHUNK_SIZE octets (donnees en clair).
    pub chunks: Vec<[u8; 32]>,
}

impl FileEntry {
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&(self.rel_path.len() as u32).to_be_bytes());
        v.extend_from_slice(self.rel_path.as_bytes());
        v.extend_from_slice(&self.size.to_be_bytes());
        v.extend_from_slice(&self.hash);
        v.extend_from_slice(&(self.chunks.len() as u32).to_be_bytes());
        for c in &self.chunks {
            v.extend_from_slice(c);
        }
        v
    }

    pub fn decode(data: &[u8]) -> Result<(Self, &[u8]), String> {
        if data.len() < 4 {
            return Err("tampon trop court".into());
        }
        let plen = u32::from_be_bytes(data[0..4].try_into().unwrap()) as usize;
        let need = plen
            .checked_add(48)
            .ok_or_else(|| "chemin trop long".to_string())?;
        if data.len() < need {
            return Err("entree tronquee".into());
        }
        let rel_path = String::from_utf8(data[4..4 + plen].to_vec())
            .map_err(|_| "chemin non-UTF8".to_string())?;
        let off = 4 + plen;
        let size = u64::from_be_bytes(data[off..off + 8].try_into().unwrap());
        let hash = data[off + 8..off + 40].try_into().unwrap();
        let n = u32::from_be_bytes(data[off + 40..off + 44].try_into().unwrap()) as usize;
        let chunks_off = off + 44;
        let need2 = n
            .checked_mul(32)
            .and_then(|v| chunks_off.checked_add(v))
            .ok_or_else(|| "trop de morceaux".to_string())?;
        if data.len() < need2 {
            return Err("morceaux tronques".into());
        }
        let mut chunks = Vec::with_capacity(n.min(1 << 20));
        for i in 0..n {
            chunks.push(
                data[chunks_off + i * 32..chunks_off + i * 32 + 32]
                    .try_into()
                    .unwrap(),
            );
        }
        Ok((
            FileEntry {
                rel_path,
                size,
                hash,
                chunks,
            },
            &data[need2..],
        ))
    }
}

pub const KIND_META: u8 = 1;
pub const KIND_CHUNK: u8 = 2;

pub fn write_kind<S: Write>(s: &mut S, kind: u8, msg: &[u8]) -> std::io::Result<()> {
    let mut hdr = [0u8; 9];
    hdr[..4].copy_from_slice(&MAGIC);
    hdr[4] = kind;
    hdr[5..].copy_from_slice(&(msg.len() as u32).to_be_bytes());
    s.write_all(&hdr)?;
    s.write_all(msg)?;
    Ok(())
}

pub fn write_msg<S: Write>(s: &mut S, msg: &[u8]) -> std::io::Result<()> {
    write_kind(s, KIND_META, msg)
}

pub fn read_exact<S: Read>(s: &mut S, n: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0u8; n];
    s.read_exact(&mut buf)?;
    Ok(buf)
}

pub fn read_kind<S: Read>(s: &mut S) -> std::io::Result<(u8, Vec<u8>)> {
    let hdr = read_exact(s, 9)?;
    if hdr[..4] != MAGIC {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "signature de protocole invalide",
        ));
    }
    let len = u32::from_be_bytes(hdr[5..].try_into().unwrap()) as usize;
    if len > 512 * 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "message trop volumineux",
        ));
    }
    Ok((hdr[4], read_exact(s, len)?))
}

pub fn read_msg<S: Read>(s: &mut S) -> std::io::Result<Vec<u8>> {
    let (kind, msg) = read_kind(s)?;
    if kind != KIND_META {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "type de message inattendu",
        ));
    }
    Ok(msg)
}

pub struct Manifest {
    pub root_name: String,
    pub files: Vec<FileEntry>,
    pub flags: u8,
}

impl Manifest {
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&(self.root_name.len() as u32).to_be_bytes());
        v.extend_from_slice(self.root_name.as_bytes());
        v.push(self.flags);
        v.extend_from_slice(&(self.files.len() as u32).to_be_bytes());
        for f in &self.files {
            v.extend_from_slice(&f.encode());
        }
        v
    }

    pub fn decode(data: &[u8]) -> Result<Self, String> {
        if data.len() < 4 {
            return Err("manifeste tronque".into());
        }
        let nlen = u32::from_be_bytes(data[0..4].try_into().unwrap()) as usize;
        let need = nlen
            .checked_add(9)
            .ok_or_else(|| "nom trop long".to_string())?;
        if data.len() < need {
            return Err("manifeste tronque".into());
        }
        let root_name = String::from_utf8(data[4..4 + nlen].to_vec())
            .map_err(|_| "nom de racine non-UTF8".to_string())?;
        let flags = data[4 + nlen];
        let count = u32::from_be_bytes(data[5 + nlen..9 + nlen].try_into().unwrap()) as usize;
        let mut files = Vec::with_capacity(count.min(1_000_000));
        let mut rest = &data[9 + nlen..];
        for _ in 0..count {
            let (f, r) = FileEntry::decode(rest)?;
            files.push(f);
            rest = r;
        }
        Ok(Manifest {
            root_name,
            files,
            flags,
        })
    }
}

/// Correspondance simple : `*` = toute sequence, `?` = un caractere.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    fn inner(p: &[u8], t: &[u8]) -> bool {
        if p.is_empty() {
            return t.is_empty();
        }
        match p[0] {
            b'*' => inner(&p[1..], t) || (!t.is_empty() && inner(p, &t[1..])),
            b'?' => !t.is_empty() && inner(&p[1..], &t[1..]),
            c => !t.is_empty() && t[0] == c && inner(&p[1..], &t[1..]),
        }
    }
    inner(pattern.as_bytes(), text.as_bytes())
}

/// Un motif sans `/` s'applique aussi a chaque composant du chemin
/// (ex. `node_modules` exclut le dossier et tout son contenu).
pub fn is_excluded(rel: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| {
        let p = p.trim();
        if p.is_empty() {
            return false;
        }
        if glob_match(p, rel) {
            return true;
        }
        if !p.contains('/') {
            return rel.split('/').any(|c| glob_match(p, c));
        }
        false
    })
}

/// Nom de peripherique reserve Windows (CON, PRN, AUX, NUL, COM1-9, LPT1-9).
fn is_windows_device(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or("");
    let u = stem.to_ascii_uppercase();
    ["CON", "PRN", "AUX", "NUL"].contains(&u.as_str())
        || ((u.starts_with("COM") || u.starts_with("LPT"))
            && u.len() == 4
            && u[3..].chars().all(|c| c.is_ascii_digit()))
}

/// Convertit un chemin relatif recu en chemin local sur.
/// Rejette `..`, les composants vides, tout composant contenant `:` ou `\`
/// (echappe les lettres de lecteur Windows comme `C:` et les chemins UNC),
/// ainsi que les noms de peripheriques reserves Windows.
pub fn rel_to_os(rel: &str) -> PathBuf {
    let mut p = PathBuf::new();
    for part in rel.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            continue;
        }
        if part.contains(':') || part.contains('\\') || is_windows_device(part) {
            continue;
        }
        p.push(part);
    }
    p
}

/// Parcourt `root` sans suivre les liens symboliques (evite les fuites de
/// fichiers hors de la racine et les boucles de liens), applique les motifs
/// d'exclusion, et calcule le hash global + le hash de chaque morceau.
/// Retourne (fichiers, nombre de liens ignores).
pub fn scan_dir<F: Fn(usize, usize, &str) + Send + Sync>(
    root: &std::path::Path,
    exclude: &[String],
    on_file: F,
) -> Result<(Vec<FileEntry>, usize), String> {
    on_file(0, 0, "parcours des sous-dossiers...");
    let mut paths = Vec::new();
    let mut skipped_links = 0usize;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries =
            std::fs::read_dir(&dir).map_err(|e| format!("lecture de {} : {}", dir.display(), e))?;
        let mut items: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        items.sort();
        for path in items {
            let ft = match std::fs::symlink_metadata(&path) {
                Ok(m) => m.file_type(),
                Err(_) => continue,
            };
            if ft.is_symlink() {
                skipped_links += 1;
            } else if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() {
                paths.push(path);
            }
        }
    }

    let mut sized: Vec<(PathBuf, String, u64)> = Vec::with_capacity(paths.len());
    for path in paths {
        let rel = path.strip_prefix(root).unwrap();
        let rel_path = rel.to_string_lossy().replace('\\', "/");
        if is_excluded(&rel_path, exclude) {
            continue;
        }
        let meta = std::fs::metadata(&path)
            .map_err(|e| format!("metadonnees de {} : {}", path.display(), e))?;
        sized.push((path, rel_path, meta.len()));
    }

    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .max(1);
    let chunk_size = sized.len().div_ceil(workers).max(1);
    let done = std::sync::atomic::AtomicUsize::new(0);
    let cb = &on_file;
    let done_ref = &done;
    let total_files = sized.len();
    let results: Vec<Result<Vec<FileEntry>, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = sized
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    let mut out = Vec::with_capacity(chunk.len());
                    for (path, rel_path, len) in chunk {
                        cb(
                            done_ref.load(std::sync::atomic::Ordering::Relaxed),
                            total_files,
                            rel_path,
                        );
                        if *len > 0 {
                            let mut f = std::fs::File::open(path)
                                .map_err(|e| format!("ouverture de {} : {}", path.display(), e))?;
                            let mut hasher = sha2::Sha256::new();
                            let mut chunks: Vec<[u8; 32]> = Vec::new();
                            let mut chunk = vec![0u8; 1024 * 1024];
                            loop {
                                let mut filled = 0usize;
                                while filled < chunk.len() {
                                    let n = f.read(&mut chunk[filled..]).map_err(|e| {
                                        format!("lecture de {} : {}", path.display(), e)
                                    })?;
                                    if n == 0 {
                                        break;
                                    }
                                    filled += n;
                                }
                                if filled == 0 {
                                    break;
                                }
                                hasher.update(&chunk[..filled]);
                                chunks.push(sha2::Sha256::digest(&chunk[..filled]).into());
                            }
                            out.push(FileEntry {
                                rel_path: rel_path.clone(),
                                size: *len,
                                hash: hasher.finalize().into(),
                                chunks,
                            });
                        } else {
                            out.push(FileEntry {
                                rel_path: rel_path.clone(),
                                size: 0,
                                hash: sha2::Sha256::digest([]).into(),
                                chunks: Vec::new(),
                            });
                        }
                        done_ref.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    Ok(out)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let mut files = Vec::with_capacity(sized.len());
    for r in results {
        files.extend(r?);
    }
    files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok((files, skipped_links))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_entry_decode_sans_panic_sur_tronque() {
        // plen = 4 mais tampon trop court pour size+hash+chunks : doit renvoyer
        // une erreur, pas un panic d'index.
        let mut data = Vec::new();
        data.extend_from_slice(&4u32.to_be_bytes());
        data.extend_from_slice(b"a.txt");
        data.extend_from_slice(&[0u8; 10]);
        assert!(FileEntry::decode(&data).is_err());
        assert!(FileEntry::decode(&[]).is_err());
        assert!(FileEntry::decode(&[0, 0]).is_err());
    }

    #[test]
    fn file_entry_roundtrip_avec_chunks() {
        let e = FileEntry {
            rel_path: "sous/fichier.bin".into(),
            size: 3 * 1024 * 1024 + 7,
            hash: [7u8; 32],
            chunks: vec![[1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32]],
        };
        let enc = e.encode();
        let (d, rest) = FileEntry::decode(&enc).unwrap();
        assert_eq!(d, e);
        assert!(rest.is_empty());
    }

    #[test]
    fn manifest_roundtrip_flags() {
        let m = Manifest {
            root_name: "dossier".into(),
            files: vec![FileEntry {
                rel_path: "a".into(),
                size: 0,
                hash: [0u8; 32],
                chunks: vec![],
            }],
            flags: FLAG_COMPRESSED,
        };
        let (d, _) = {
            let enc = m.encode();
            (Manifest::decode(&enc).unwrap(), enc)
        };
        assert_eq!(d.root_name, "dossier");
        assert_eq!(d.flags, FLAG_COMPRESSED);
        assert_eq!(d.files.len(), 1);
    }

    #[test]
    fn rel_to_os_rejette_traversals_et_lecteurs() {
        assert_eq!(rel_to_os("a/b.txt"), PathBuf::from("a").join("b.txt"));
        assert_eq!(
            rel_to_os("../../etc/passwd"),
            PathBuf::from("etc").join("passwd")
        );
        assert_eq!(
            rel_to_os("C:/Windows/system32"),
            PathBuf::from("Windows").join("system32")
        );
        assert_eq!(rel_to_os("\\\\serveur\\partage"), PathBuf::new());
        assert_eq!(
            rel_to_os("/etc/passwd"),
            PathBuf::from("etc").join("passwd")
        );
        assert_eq!(rel_to_os("CON"), PathBuf::new());
        assert_eq!(rel_to_os("dir/nul.txt"), PathBuf::from("dir"));
        assert_eq!(
            rel_to_os("a/./b/../c"),
            PathBuf::from("a").join("b").join("c")
        );
    }

    #[test]
    fn glob_match_simple() {
        assert!(glob_match("*.tmp", "fichier.tmp"));
        assert!(!glob_match("*.tmp", "fichier.txt"));
        assert!(glob_match("sous/*", "sous/a/b.txt"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(is_excluded(
            "node_modules/x.js",
            &["node_modules".to_string(), "*.tmp".to_string()]
        ));
        assert!(is_excluded("build/out.tmp", &["*.tmp".to_string()]));
        assert!(!is_excluded("src/main.rs", &["*.tmp".to_string()]));
    }
}

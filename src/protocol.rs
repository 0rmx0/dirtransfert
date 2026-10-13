use sha2::Digest;
use std::io::{Read, Write};
use std::path::PathBuf;

pub const MAGIC: [u8; 4] = *b"DTX1";
#[allow(dead_code)]
pub const CHUNK_SIZE: usize = 1024 * 1024;
#[allow(dead_code)]
pub const DEFAULT_PORT: u16 = 47500;

#[derive(Debug, Clone, PartialEq)]
pub struct FileEntry {
    pub rel_path: String,
    pub size: u64,
    pub hash: [u8; 32],
}

impl FileEntry {
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&(self.rel_path.len() as u32).to_be_bytes());
        v.extend_from_slice(self.rel_path.as_bytes());
        v.extend_from_slice(&self.size.to_be_bytes());
        v.extend_from_slice(&self.hash);
        v
    }

    pub fn decode(data: &[u8]) -> Result<(Self, &[u8]), String> {
        if data.len() < 8 {
            return Err("tampon trop court".into());
        }
        let plen = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if data.len() < 8 + plen {
            return Err("chemin tronque".into());
        }
        let rel_path = String::from_utf8(data[4..4 + plen].to_vec())
            .map_err(|_| "chemin non-UTF8".to_string())?;
        let off = 4 + plen;
        let size = u64::from_be_bytes(data[off..off + 8].try_into().unwrap());
        let hash = data[off + 8..off + 40].try_into().unwrap();
        Ok((FileEntry { rel_path, size, hash }, &data[off + 40..]))
    }
}

pub fn write_msg<S: Write>(s: &mut S, msg: &[u8]) -> std::io::Result<()> {
    let mut hdr = [0u8; 8];
    hdr[..4].copy_from_slice(&MAGIC);
    hdr[4..].copy_from_slice(&(msg.len() as u32).to_be_bytes());
    s.write_all(&hdr)?;
    s.write_all(msg)?;
    Ok(())
}

pub fn read_exact<S: Read>(s: &mut S, n: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0u8; n];
    s.read_exact(&mut buf)?;
    Ok(buf)
}

pub fn read_msg<S: Read>(s: &mut S) -> std::io::Result<Vec<u8>> {
    let hdr = read_exact(s, 8)?;
    if hdr[..4] != MAGIC {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "signature de protocole invalide",
        ));
    }
    let len = u32::from_be_bytes(hdr[4..].try_into().unwrap()) as usize;
    if len > 512 * 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "message trop volumineux",
        ));
    }
    read_exact(s, len)
}

pub struct Manifest {
    pub root_name: String,
    pub files: Vec<FileEntry>,
}

impl Manifest {
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&(self.root_name.len() as u32).to_be_bytes());
        v.extend_from_slice(self.root_name.as_bytes());
        v.extend_from_slice(&(self.files.len() as u32).to_be_bytes());
        for f in &self.files {
            v.extend_from_slice(&f.encode());
        }
        v
    }

    pub fn decode(data: &[u8]) -> Result<Self, String> {
        if data.len() < 8 {
            return Err("manifeste tronque".into());
        }
        let nlen = u32::from_be_bytes(data[0..4].try_into().unwrap()) as usize;
        if data.len() < 4 + nlen + 4 {
            return Err("manifeste tronque".into());
        }
        let root_name = String::from_utf8(data[4..4 + nlen].to_vec())
            .map_err(|_| "nom de racine non-UTF8".to_string())?;
        let count = u32::from_be_bytes(data[4 + nlen..8 + nlen].try_into().unwrap()) as usize;
        let mut files = Vec::with_capacity(count.min(1_000_000));
        let mut rest = &data[8 + nlen..];
        for _ in 0..count {
            let (f, r) = FileEntry::decode(rest)?;
            files.push(f);
            rest = r;
        }
        Ok(Manifest { root_name, files })
    }
}

pub fn scan_dir(root: &std::path::Path) -> Result<Vec<FileEntry>, String> {
    let mut paths = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("lecture de {} : {}", dir.display(), e))?;
        let mut items: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        items.sort();
        for path in items {
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                paths.push(path);
            }
        }
    }

    let mut sized: Vec<(PathBuf, String, u64)> = Vec::with_capacity(paths.len());
    for path in paths {
        let rel = path.strip_prefix(root).unwrap();
        let rel_path = rel.to_string_lossy().replace('\\', "/");
        let meta = std::fs::metadata(&path).map_err(|e| format!("metadonnees de {} : {}", path.display(), e))?;
        sized.push((path, rel_path, meta.len()));
    }

    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .max(1);
    let chunk_size = sized.len().div_ceil(workers);
    let results: Vec<Result<Vec<FileEntry>, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = sized
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    let mut out = Vec::with_capacity(chunk.len());
                    for (path, rel_path, len) in chunk {
                        if *len > 0 {
                            let mut f = std::fs::File::open(path).map_err(|e| format!("ouverture de {} : {}", path.display(), e))?;
                            let mut hasher = sha2::Sha256::new();
                            let mut buf = vec![0u8; 1024 * 1024];
                            loop {
                                let n = f.read(&mut buf).map_err(|e| format!("lecture de {} : {}", path.display(), e))?;
                                if n == 0 { break; }
                                hasher.update(&buf[..n]);
                            }
                            out.push(FileEntry { rel_path: rel_path.clone(), size: *len, hash: hasher.finalize().into() });
                        } else {
                            out.push(FileEntry { rel_path: rel_path.clone(), size: 0, hash: sha2::Sha256::digest(&[]).into() });
                        }
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
    Ok(files)
}

pub fn rel_to_os(rel: &str) -> PathBuf {
    let mut p = PathBuf::new();
    for part in rel.split('/') {
        if part != ".." && part != "." && !part.is_empty() {
            p.push(part);
        }
    }
    p
}

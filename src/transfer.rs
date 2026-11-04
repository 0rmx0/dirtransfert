use crate::crypto::SecureStream;
use crate::discover;
use crate::firewall::RULE_NAME;
use crate::protocol::*;
use crate::{firewall, net, upnp};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{IpAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const PIPELINE_WINDOW: usize = 8;
const IO_TIMEOUT: Duration = Duration::from_secs(30);
const HANDSHAKE_MAX_ATTEMPTS: usize = 5;
const HANDSHAKE_WINDOW: Duration = Duration::from_secs(60);

pub fn human(bytes: u64) -> String {
    let units = ["o", "Ko", "Mo", "Go", "To"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < 4 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} {}", bytes, units[i])
    } else {
        format!("{:.2} {}", v, units[i])
    }
}

fn file_hash(path: &Path) -> Result<[u8; 32], String> {
    let mut f = std::fs::File::open(path)
        .map_err(|e| format!("ouverture de {} : {}", path.display(), e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK_SIZE];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("lecture de {} : {}", path.display(), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().into())
}

// ---- Requetes du recepteur (meta) : [u8 kind][u32 plen][chemin][u64 start_chunk]
pub const REQ_FULL: u8 = 0;
pub const REQ_RESUME: u8 = 1;

pub fn encode_request(rel: &str, start_chunk: u64) -> Vec<u8> {
    let kind = if start_chunk > 0 {
        REQ_RESUME
    } else {
        REQ_FULL
    };
    let mut v = Vec::with_capacity(13 + rel.len());
    v.push(kind);
    v.extend_from_slice(&(rel.len() as u32).to_be_bytes());
    v.extend_from_slice(rel.as_bytes());
    v.extend_from_slice(&start_chunk.to_be_bytes());
    v
}

pub fn decode_request(data: &[u8]) -> Result<(String, u64), String> {
    if data.len() < 13 {
        return Err("requete tronquee".into());
    }
    let kind = data[0];
    if kind != REQ_FULL && kind != REQ_RESUME {
        return Err("type de requete inconnu".into());
    }
    let plen = u32::from_be_bytes(data[1..5].try_into().unwrap()) as usize;
    let need = plen
        .checked_add(13)
        .ok_or_else(|| "chemin trop long".to_string())?;
    if data.len() < need {
        return Err("requete tronquee".into());
    }
    let rel = String::from_utf8(data[5..5 + plen].to_vec())
        .map_err(|_| "chemin de requete non-UTF8".to_string())?;
    let start = u64::from_be_bytes(data[5 + plen..13 + plen].try_into().unwrap());
    if kind == REQ_FULL && start != 0 {
        return Err("requete incoherente".into());
    }
    Ok((rel, start))
}

#[cfg(feature = "compress")]
fn compress_chunk(data: &[u8]) -> Result<Vec<u8>, String> {
    zstd::bulk::compress(data, 3).map_err(|e| format!("compression : {}", e))
}

#[cfg(not(feature = "compress"))]
fn compress_chunk(_data: &[u8]) -> Result<Vec<u8>, String> {
    Err("compression demandee mais binaire compile sans la feature compress".into())
}

#[cfg(feature = "compress")]
fn decompress_chunk(data: &[u8]) -> Result<Vec<u8>, String> {
    zstd::bulk::decompress(data, CHUNK_SIZE).map_err(|e| format!("decompression : {}", e))
}

#[cfg(not(feature = "compress"))]
fn decompress_chunk(_data: &[u8]) -> Result<Vec<u8>, String> {
    Err("morceau compresse recu mais binaire compile sans la feature compress".into())
}

/// Chemin du fichier temporaire de reception (suffixe `.dtxpart`, le nom
/// complet est conserve pour eviter les collisions entre `a.txt` et `a.bin`).
pub fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_os_string();
    s.push(".dtxpart");
    PathBuf::from(s)
}

fn write_empty_atomically(dest: &Path, expected_hash: [u8; 32]) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creation de {} : {}", parent.display(), e))?;
    }
    let tmp = part_path(dest);
    std::fs::write(&tmp, b"").map_err(|e| format!("creation de {} : {}", tmp.display(), e))?;
    let got: [u8; 32] = Sha256::digest([]).into();
    if got != expected_hash {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("checksum invalide pour {}", dest.display()));
    }
    std::fs::rename(&tmp, dest)
        .map_err(|e| format!("renommage vers {} : {}", dest.display(), e))?;
    Ok(())
}

/// Recoit un fichier (complet ou reprise depuis `start_chunk`) en verifiant le
/// hash de chaque morceau. Le fichier temporaire `.dtxpart` est conserve en
/// cas d'erreur reseau pour permettre une reprise ulterieure.
#[allow(clippy::too_many_arguments)]
fn receive_file(
    stream: &mut SecureStream,
    dest: &Path,
    entry: &FileEntry,
    start_chunk: u64,
    compressed: bool,
    cancel: &Option<Arc<AtomicBool>>,
    rep: &dyn Reporter,
    sent: &AtomicU64,
    total: u64,
    start: &Instant,
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creation de {} : {}", parent.display(), e))?;
    }
    let tmp = part_path(dest);
    let start_off = start_chunk
        .checked_mul(CHUNK_SIZE as u64)
        .ok_or_else(|| "offset de reprise trop grand".to_string())?;
    if start_off > entry.size {
        return Err(format!(
            "{} : offset de reprise hors du fichier",
            entry.rel_path
        ));
    }
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&tmp)
        .map_err(|e| format!("ouverture de {} : {}", tmp.display(), e))?;
    f.set_len(start_off)
        .map_err(|e| format!("troncature de {} : {}", tmp.display(), e))?;
    let mut hasher = Sha256::new();
    let mut total_written = 0u64;
    if start_off > 0 {
        f.seek(SeekFrom::Start(0))
            .map_err(|e| format!("seek de {} : {}", tmp.display(), e))?;
        let mut buf = vec![0u8; CHUNK_SIZE];
        let mut remaining = start_off;
        while remaining > 0 {
            let want = remaining.min(CHUNK_SIZE as u64) as usize;
            let n = f
                .read(&mut buf[..want])
                .map_err(|e| format!("lecture de {} : {}", tmp.display(), e))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            total_written += n as u64;
            remaining -= n as u64;
        }
        if total_written != start_off {
            return Err(format!(
                "{} : .dtxpart illisible en reprise",
                entry.rel_path
            ));
        }
    }
    f.seek(SeekFrom::Start(start_off))
        .map_err(|e| format!("seek de {} : {}", tmp.display(), e))?;
    let mut chunk_idx = start_chunk as usize;
    let flow = (|| -> Result<(), String> {
        loop {
            check_cancel(cancel)?;
            let (kind, data) = read_kind(stream)
                .map_err(|e| format!("reception de {} : {}", entry.rel_path, e))?;
            match kind {
                KIND_CHUNK => {
                    if data.is_empty() {
                        return Err(format!("{} : morceau vide", entry.rel_path));
                    }
                    let plain: Vec<u8> = if compressed {
                        decompress_chunk(&data)?
                    } else {
                        data
                    };
                    let expect = entry.chunks.get(chunk_idx).ok_or_else(|| {
                        format!("{} : morceau {} inattendu", entry.rel_path, chunk_idx)
                    })?;
                    if Sha256::digest(&plain).as_slice() != expect {
                        return Err(format!(
                            "{} : morceau {} invalide",
                            entry.rel_path, chunk_idx
                        ));
                    }
                    f.write_all(&plain)
                        .map_err(|e| format!("ecriture de {} : {}", tmp.display(), e))?;
                    hasher.update(&plain);
                    total_written = total_written.wrapping_add(plain.len() as u64);
                    chunk_idx += 1;
                    sent.fetch_add(plain.len() as u64, Ordering::Relaxed);
                    progress_report(rep, sent, total, start);
                }
                KIND_META if data.is_empty() => break,
                _ => return Err(format!("{} : type de trame inattendu", entry.rel_path)),
            }
        }
        Ok(())
    })();
    flow?;
    if chunk_idx != entry.chunks.len() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "{} : {} morceaux recus sur {} attendus",
            entry.rel_path,
            chunk_idx,
            entry.chunks.len()
        ));
    }
    if total_written != entry.size {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "taille invalide pour {} ({} octets attendus)",
            entry.rel_path, entry.size
        ));
    }
    let got: [u8; 32] = hasher.finalize().into();
    if got != entry.hash {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("checksum invalide pour {}", entry.rel_path));
    }
    f.sync_all()
        .map_err(|e| format!("sync de {} : {}", tmp.display(), e))?;
    drop(f);
    std::fs::rename(&tmp, dest)
        .map_err(|e| format!("renommage vers {} : {}", dest.display(), e))?;
    Ok(())
}

/// Nombre de morceaux valides au debut d'un `.dtxpart` existant (0 si absent).
fn resume_start_chunk(entry: &FileEntry, tmp: &Path) -> u64 {
    if !tmp.is_file() {
        return 0;
    }
    let Ok(meta) = std::fs::metadata(tmp) else {
        return 0;
    };
    let len = meta.len();
    if len == 0 || entry.chunks.is_empty() {
        return 0;
    }
    let Ok(mut f) = std::fs::File::open(tmp) else {
        return 0;
    };
    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut k = 0u64;
    loop {
        let mut filled = 0usize;
        while filled < CHUNK_SIZE {
            match f.read(&mut buf[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(_) => return 0,
            }
        }
        if filled == 0 || k as usize >= entry.chunks.len() {
            break;
        }
        if Sha256::digest(&buf[..filled]).as_slice() != entry.chunks[k as usize] {
            break;
        }
        k += 1;
        if filled < CHUNK_SIZE {
            break;
        }
    }
    k
}

fn load_existing_hashes(dest_root: &Path, manifest: &Manifest) -> HashMap<String, [u8; 32]> {
    let mut done = HashMap::new();
    for entry in &manifest.files {
        let p = dest_root.join(rel_to_os(&entry.rel_path));
        if p.is_file() {
            if let Ok(meta) = std::fs::metadata(&p) {
                if meta.len() == entry.size {
                    if let Ok(h) = file_hash(&p) {
                        done.insert(entry.rel_path.clone(), h);
                    }
                }
            }
        }
    }
    done
}

pub fn parse_endpoint(s: &str) -> Option<(String, u16)> {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        let port = after.strip_prefix(':')?;
        if port.is_empty() || port.contains(']') {
            return None;
        }
        return port.parse::<u16>().ok().map(|p| (host.to_string(), p));
    }
    let (host, port) = trimmed.rsplit_once(':')?;
    if host.is_empty() || host.contains(':') {
        return None;
    }
    port.parse::<u16>().ok().map(|p| (host.to_string(), p))
}

#[derive(Default, Clone)]
pub struct Opts {
    pub upnp: bool,
    pub open_fw: bool,
    pub close_fw_after: bool,
    pub pipeline: Option<usize>,
    pub prefer_ipv6: bool,
    pub cancel: Option<Arc<AtomicBool>>,
    pub auto_yes: bool,
    pub exclude: Vec<String>,
    pub dry_run: bool,
    pub limit_mbps: Option<f64>,
    pub compress: bool,
}

pub const CANCELLED_MSG: &str = "transfert annule par l'utilisateur";

fn cancelled(cancel: &Option<Arc<AtomicBool>>) -> bool {
    cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed))
}

fn check_cancel(cancel: &Option<Arc<AtomicBool>>) -> Result<(), String> {
    if cancelled(cancel) {
        Err(CANCELLED_MSG.into())
    } else {
        Ok(())
    }
}

pub trait Reporter: Send + Sync {
    fn log(&self, msg: String);
    fn scan(&self, _done: usize, _total: usize, _current: &str) {}
    fn progress(&self, _sent: u64, _total: u64, _speed_mbps: f64, _pct: u64) {}
    fn done(&self, _ok: bool, _files_ok: usize, _files_total: usize) {}
    /// Approuve (ou non) la reception d'un manifeste. Defaut : oui.
    fn confirm_manifest(&self, _root_name: &str, _files: &[(String, u64)], _total: u64) -> bool {
        true
    }
}

pub struct NullReporter;
impl Reporter for NullReporter {
    fn log(&self, _msg: String) {}
    fn scan(&self, _d: usize, _t: usize, _c: &str) {}
    fn progress(&self, _s: u64, _t: u64, _sp: f64, _p: u64) {}
    fn done(&self, _ok: bool, _n: usize, _tot: usize) {}
}

fn progress_report(rep: &dyn Reporter, sent: &AtomicU64, total: u64, start: &Instant) {
    let s = sent.load(Ordering::Relaxed);
    let el = start.elapsed().as_secs_f64();
    let speed = if el > 0.0 { s as f64 / el } else { 0.0 };
    let pct = (s * 100).checked_div(total).unwrap_or(100);
    rep.progress(s, total, speed / 1024.0 / 1024.0, pct);
}

fn throttle_limit(opts: &Opts, sent: &AtomicU64, start: &Instant) {
    let Some(mbps) = opts.limit_mbps else { return };
    let limit = mbps * 1024.0 * 1024.0;
    if limit <= 0.0 {
        return;
    }
    let el = start.elapsed().as_secs_f64();
    let expected = sent.load(Ordering::Relaxed) as f64 / limit;
    if expected > el {
        std::thread::sleep(Duration::from_secs_f64(expected - el));
    }
}

pub fn receive(
    addr: &str,
    out_dir: &Path,
    code: &str,
    opts: &Opts,
    rep: &dyn Reporter,
) -> Result<(), String> {
    let Some((_, port)) = parse_endpoint(addr) else {
        return Err(format!("adresse \"{}\" invalide (attendu hote:port)", addr));
    };
    if opts.open_fw {
        match firewall::ensure_firewall_rule(port) {
            Ok(_) => rep.log(format!(
                "regle pare-feu \"{}\" ouverte sur le port TCP {}",
                RULE_NAME, port
            )),
            Err(e) => rep.log(format!(
                "pare-feu : {} (ouvrez le port {} manuellement)",
                e, port
            )),
        }
    }
    let mut upnp_map = None;
    let mut natpmp_used = false;
    let mut bind_addr = addr.to_string();
    if opts.prefer_ipv6 {
        if let Some(ip6) = net::global_ipv6() {
            rep.log(format!("IPv6 globale disponible : {}", ip6));
            let Some((_, v6port)) = parse_endpoint(addr) else {
                return Err(format!("adresse \"{}\" invalide (attendu hote:port)", addr));
            };
            bind_addr = format!("[{}]:{}", ip6, v6port);
            rep.log(format!(
                "sur le PC emetteur (via Internet), renseignez : [{}]:{}",
                ip6, v6port
            ));
        } else {
            rep.log("IPv6 indisponible sur ce reseau, bascule sur IPv4".to_string());
        }
    }
    if opts.upnp && bind_addr == addr {
        match upnp::add_port_mapping(port) {
            Ok(m) => {
                rep.log(format!(
                    "redirection UPnP active : port TCP {} -> ce PC",
                    m.external_port
                ));
                let ext = if m.external_ip.is_empty() {
                    String::from("<IP_publique_de_la_box>")
                } else {
                    m.external_ip.clone()
                };
                rep.log(format!(
                    "sur l'emetteur, lancez : DirTransfert --cli send <repertoire> {}:{} --code {}",
                    ext, m.external_port, code
                ));
                upnp_map = Some(m);
            }
            Err(e) => {
                rep.log(format!("UPnP : {} (tentative NAT-PMP en cours)", e));
                match net::natpmp_add_mapping(port) {
                    Ok(gw) => {
                        natpmp_used = true;
                        rep.log(format!(
                            "redirection NAT-PMP active : port TCP {} -> ce PC (passerelle {})",
                            port, gw
                        ));
                        rep.log(format!(
                            "sur l'emetteur, lancez : DirTransfert --cli send <repertoire> <IP_publique_de_la_box>:{} --code {}",
                            port, code
                        ));
                    }
                    Err(e2) => rep.log(format!(
                        "NAT-PMP : {} (configurez la redirection manuellement sur la box)",
                        e2
                    )),
                }
            }
        }
    }
    let ip_locale = upnp::local_ip();
    if !ip_locale.is_empty() {
        rep.log(format!("IP locale de ce PC : {}", ip_locale));
    }
    match net::detect_net_mode() {
        Ok((mode, ip_pub)) => {
            rep.log(format!("IP publique : {} ({})", ip_pub, mode.label()));
            if bind_addr == addr {
                rep.log(format!(
                    "sur le PC emetteur (meme reseau), renseignez : {}:{}",
                    ip_locale, port
                ));
                rep.log(format!(
                    "sur le PC emetteur (via Internet), renseignez : {}:{}",
                    ip_pub, port
                ));
            }
            if mode == net::NetMode::Cgnat {
                rep.log("attention : IP en plage 100.64-127.x.x = CGNAT operateur, la redirection de port ne suffira pas, utilisez un VPN (Tailscale/WireGuard) ou IPv6".to_string());
            }
        }
        Err(e) => {
            if !ip_locale.is_empty() {
                rep.log(format!(
                    "sur le PC emetteur (meme reseau), renseignez : {}:{}",
                    ip_locale, port
                ));
            }
            rep.log(format!("IP publique : {} (via Internet, trouvez l'IP de la box sur son interface d'administration)", e));
        }
    }
    let listener =
        TcpListener::bind(&bind_addr).map_err(|e| format!("liaison de {} : {}", bind_addr, e))?;
    rep.log(format!("en ecoute sur {}", bind_addr));
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("mode non bloquant : {}", e))?;
    let disc = discover::responder();
    if disc.is_some() {
        rep.log(format!(
            "decouverte LAN active : un emetteur avec le meme code peut vous trouver (UDP {})",
            discover::DISCOVERY_PORT
        ));
    }
    // Boucle d'acceptation avec limitation des tentatives de poignee de main par IP.
    let mut attempts: HashMap<IpAddr, VecDeque<Instant>> = HashMap::new();
    let (mut stream, _peer) = loop {
        check_cancel(&opts.cancel)?;
        if let Some(d) = &disc {
            while let Some(msg) = d.poll(code, port) {
                rep.log(msg);
            }
        }
        match listener.accept() {
            Ok((sock, peer)) => {
                let now = Instant::now();
                let q = attempts.entry(peer.ip()).or_default();
                q.retain(|t| now.duration_since(*t) < HANDSHAKE_WINDOW);
                if q.len() >= HANDSHAKE_MAX_ATTEMPTS {
                    rep.log(format!(
                        "{} : trop de tentatives de poignee de main en 60 s, connexion ignoree",
                        peer
                    ));
                    continue;
                }
                q.push_back(now);
                let _ = sock.set_nonblocking(false);
                rep.log(format!("connexion de {}", peer));
                match SecureStream::server_handshake(sock, code) {
                    Ok(s) => break (s, peer),
                    Err(e) => {
                        rep.log(format!(
                            "poignee de main refusee avec {} : {} — en attente d'une nouvelle connexion",
                            peer, e
                        ));
                        continue;
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(format!("acceptation : {}", e)),
        }
    };
    stream.set_timeouts(Some(IO_TIMEOUT), Some(IO_TIMEOUT));
    rep.log("flux chiffre (ChaCha20-Poly1305, code de jumelage verifie)".to_string());

    let msg = read_msg(&mut stream).map_err(|e| format!("reception du manifeste : {}", e))?;
    let manifest = Manifest::decode(&msg)?;
    let compressed = manifest.flags & FLAG_COMPRESSED != 0;
    if compressed && !cfg!(feature = "compress") {
        return Err(
            "manifeste compresse recu mais binaire compile sans la feature compress".into(),
        );
    }
    let total: u64 = manifest.files.iter().map(|f| f.size).sum();
    rep.log(format!(
        "transfert de \"{}\" : {} fichiers, {}{}",
        manifest.root_name,
        manifest.files.len(),
        human(total),
        if compressed { " (compresse)" } else { "" }
    ));
    if !opts.auto_yes {
        let listing: Vec<(String, u64)> = manifest
            .files
            .iter()
            .map(|f| (f.rel_path.clone(), f.size))
            .collect();
        if !rep.confirm_manifest(&manifest.root_name, &listing, total) {
            let _ = write_msg(&mut stream, b"REFUSED");
            return Err("transfert refuse par le recepteur".into());
        }
    }

    let dest_root = out_dir.join(&manifest.root_name);
    std::fs::create_dir_all(&dest_root)
        .map_err(|e| format!("creation de {} : {}", dest_root.display(), e))?;

    let existing = load_existing_hashes(&dest_root, &manifest);
    if !existing.is_empty() {
        rep.log(format!(
            "reprise : {} fichier(s) deja valides",
            existing.len()
        ));
    }

    let sent = Arc::new(AtomicU64::new(0));
    let start = Instant::now();
    let mut verified: HashSet<String> = HashSet::new();
    let mut failures: Vec<String> = Vec::new();

    let mut plans: Vec<(&FileEntry, u64)> = Vec::new();
    for entry in &manifest.files {
        if entry.size == 0 {
            continue;
        }
        if let Some(h) = existing.get(&entry.rel_path) {
            if *h == entry.hash {
                sent.fetch_add(entry.size, Ordering::Relaxed);
                verified.insert(entry.rel_path.clone());
                continue;
            }
        }
        let dest = dest_root.join(rel_to_os(&entry.rel_path));
        let start_chunk = resume_start_chunk(entry, &part_path(&dest));
        if start_chunk > 0 {
            rep.log(format!(
                "reprise par morceaux pour {} (depuis le morceau {})",
                entry.rel_path, start_chunk
            ));
        }
        plans.push((entry, start_chunk));
    }

    for entry in manifest.files.iter().filter(|e| e.size == 0) {
        let dest = dest_root.join(rel_to_os(&entry.rel_path));
        match write_empty_atomically(&dest, entry.hash) {
            Ok(()) => {
                verified.insert(entry.rel_path.clone());
            }
            Err(e) => failures.push(format!("{} : {}", entry.rel_path, e)),
        }
    }

    let window = opts.pipeline.unwrap_or(PIPELINE_WINDOW).clamp(1, 64);
    let mut pending: VecDeque<(&FileEntry, u64)> = plans.into_iter().collect();
    let mut in_flight: VecDeque<(&FileEntry, u64)> = VecDeque::new();

    while !pending.is_empty() && in_flight.len() < window {
        let (entry, sc) = pending.pop_front().unwrap();
        write_msg(&mut stream, &encode_request(&entry.rel_path, sc))
            .map_err(|e| format!("envoi de requete pour {} : {}", entry.rel_path, e))?;
        in_flight.push_back((entry, sc));
    }

    while let Some((entry, sc)) = in_flight.pop_front() {
        if cancelled(&opts.cancel) {
            break;
        }
        let dest = dest_root.join(rel_to_os(&entry.rel_path));
        let res = receive_file(
            &mut stream,
            &dest,
            entry,
            sc,
            compressed,
            &opts.cancel,
            rep,
            &sent,
            total,
            &start,
        );
        match res {
            Ok(()) => {
                verified.insert(entry.rel_path.clone());
            }
            Err(e) => failures.push(format!("{} : {}", entry.rel_path, e)),
        }
        if !pending.is_empty() {
            let (next, nsc) = pending.pop_front().unwrap();
            write_msg(&mut stream, &encode_request(&next.rel_path, nsc))
                .map_err(|e| format!("envoi de requete pour {} : {}", next.rel_path, e))?;
            in_flight.push_back((next, nsc));
        }
    }

    let mut ok = verified.len();
    for entry in &manifest.files {
        if !verified.contains(&entry.rel_path) {
            let p = dest_root.join(rel_to_os(&entry.rel_path));
            let good = p.is_file()
                && std::fs::metadata(&p)
                    .map(|m| m.len() == entry.size)
                    .unwrap_or(false)
                && file_hash(&p).map(|h| h == entry.hash).unwrap_or(false);
            if good {
                ok += 1;
            } else {
                failures.push(format!("{} : checksum final invalide", entry.rel_path));
            }
        }
    }

    let final_status = if failures.is_empty() {
        b"OK".to_vec()
    } else {
        b"FAIL".to_vec()
    };
    write_msg(&mut stream, &final_status).map_err(|e| format!("envoi du statut final : {}", e))?;
    drop(stream);

    if let Some(m) = upnp_map {
        match upnp::delete_port_mapping(m.external_port) {
            Ok(()) => rep.log(format!(
                "redirection UPnP fermee (port {})",
                m.external_port
            )),
            Err(e) => rep.log(format!(
                "fermeture UPnP : {} (fermez le port manuellement)",
                e
            )),
        }
    }
    if natpmp_used {
        match net::natpmp_delete_mapping() {
            Ok(()) => rep.log("redirection NAT-PMP fermee".to_string()),
            Err(e) => rep.log(format!(
                "fermeture NAT-PMP : {} (fermez le port manuellement)",
                e
            )),
        }
    }
    if opts.close_fw_after {
        match firewall::remove_firewall_rule() {
            Ok(()) => rep.log(format!("regle pare-feu \"{}\" supprimee", RULE_NAME)),
            Err(e) => rep.log(format!("suppression regle pare-feu : {}", e)),
        }
    }

    let all_ok = failures.is_empty();
    rep.done(all_ok, ok, manifest.files.len());
    if !all_ok {
        let head: Vec<&str> = failures.iter().take(20).map(|s| s.as_str()).collect();
        return Err(format!(
            "transfert incomplet ({}/{} ok) : {}",
            ok,
            manifest.files.len(),
            head.join("; ")
        ));
    }
    rep.log("transfert integre, checksums verifies".to_string());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn send_one(
    dir: &Path,
    manifest: &Manifest,
    addr: &str,
    code: &str,
    opts: &Opts,
    rep: &dyn Reporter,
    sent: &AtomicU64,
    total: u64,
    start: &Instant,
) -> Result<(), String> {
    let addrs: Vec<std::net::SocketAddr> = addr
        .to_socket_addrs()
        .map_err(|e| format!("resolution de {} : {}", addr, e))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("aucune adresse trouvee pour {}", addr));
    }
    let sock = loop {
        check_cancel(&opts.cancel)?;
        let mut connected: Option<TcpStream> = None;
        for sa in &addrs {
            match TcpStream::connect_timeout(sa, Duration::from_secs(1)) {
                Ok(s) => {
                    let _ = s.set_nodelay(true);
                    connected = Some(s);
                    break;
                }
                Err(_) => continue,
            }
        }
        if let Some(s) = connected {
            break s;
        }
    };
    let mut stream = SecureStream::client_handshake(sock, code)
        .map_err(|e| format!("poignee de main chiffree avec {} : {}", addr, e))?;
    stream.set_timeouts(Some(IO_TIMEOUT), Some(IO_TIMEOUT));
    rep.log(format!(
        "connecte a {} — flux chiffre (ChaCha20-Poly1305, code de jumelage verifie)",
        addr
    ));

    write_msg(&mut stream, &manifest.encode())
        .map_err(|e| format!("envoi du manifeste : {}", e))?;

    let compressed = manifest.flags & FLAG_COMPRESSED != 0;
    let mut remaining = manifest.files.iter().filter(|f| f.size > 0).count();
    let mut buf = vec![0u8; CHUNK_SIZE];
    while remaining > 0 {
        check_cancel(&opts.cancel)?;
        let data = read_msg(&mut stream).map_err(|e| format!("protocole : {}", e))?;
        if data == b"REFUSED" {
            return Err("transfert refuse par le recepteur".into());
        }
        let (rel, start_chunk) = decode_request(&data)?;
        let ep = dir.join(rel_to_os(&rel));
        let mut f = std::fs::File::open(&ep)
            .map_err(|e| format!("ouverture de {} : {}", ep.display(), e))?;
        let start_off = start_chunk
            .checked_mul(CHUNK_SIZE as u64)
            .ok_or_else(|| "offset de reprise trop grand".to_string())?;
        f.seek(SeekFrom::Start(start_off))
            .map_err(|e| format!("seek de {} : {}", ep.display(), e))?;
        loop {
            check_cancel(&opts.cancel)?;
            let mut filled = 0usize;
            while filled < CHUNK_SIZE {
                let n = f
                    .read(&mut buf[filled..])
                    .map_err(|e| format!("lecture de {} : {}", ep.display(), e))?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
            if filled == 0 {
                break;
            }
            let payload = if compressed {
                compress_chunk(&buf[..filled])?
            } else {
                buf[..filled].to_vec()
            };
            write_kind(&mut stream, KIND_CHUNK, &payload)
                .map_err(|e| format!("envoi de {} : {}", rel, e))?;
            sent.fetch_add(filled as u64, Ordering::Relaxed);
            progress_report(rep, sent, total, start);
            throttle_limit(opts, sent, start);
        }
        write_kind(&mut stream, KIND_META, b"").map_err(|e| format!("fin de {} : {}", rel, e))?;
        remaining -= 1;
    }

    let fin = read_msg(&mut stream).map_err(|e| format!("protocole : {}", e))?;
    if fin == b"OK" {
        rep.log(format!("{} : recepteur confirme l'integrite", addr));
        rep.done(true, manifest.files.len(), manifest.files.len());
        return Ok(());
    }
    if fin == b"FAIL" {
        return Err(format!(
            "{} : le recepteur signale des checksums invalides (voir son journal)",
            addr
        ));
    }
    Err(format!(
        "statut final inattendu du recepteur : {} octets",
        fin.len()
    ))
}

/// Envoie `dir` vers une ou plusieurs adresses `hote:port`.
pub fn send(
    dir: &Path,
    addrs: &[String],
    code: &str,
    opts: &Opts,
    rep: &dyn Reporter,
) -> Result<(), String> {
    if addrs.is_empty() {
        return Err("aucune adresse de destination fournie".into());
    }
    if !dir.is_dir() {
        return Err(format!("{} n'est pas un repertoire", dir.display()));
    }
    let root_name = match dir.file_name() {
        Some(n) => n.to_string_lossy().to_string(),
        None => {
            return Err(format!(
                "impossible de determiner le nom du repertoire racine de {}",
                dir.display()
            ))
        }
    };
    check_cancel(&opts.cancel)?;
    rep.log(format!("analyse de {} ...", dir.display()));
    let last_report = Arc::new(std::sync::Mutex::new((0usize, Instant::now())));
    let scan_rep = rep;
    let scan_progress = move |done: usize, total: usize, current: &str| {
        let Ok(mut last) = last_report.lock() else {
            return;
        };
        let advance = done > last.0;
        let elapsed = last.1.elapsed().as_millis() as usize;
        if done == total || (advance && elapsed >= 200) {
            *last = (done, Instant::now());
            drop(last);
            scan_rep.scan(done, total, current);
        }
    };
    let (files, skipped_links) = scan_dir(dir, &opts.exclude, scan_progress)?;
    if skipped_links > 0 {
        rep.log(format!(
            "{} lien(s) symbolique(s) ignore(s) (non suivis)",
            skipped_links
        ));
    }
    let total: u64 = files.iter().map(|f| f.size).sum();
    rep.scan(files.len(), files.len(), "analyse terminee");
    rep.log(format!(
        "{} fichiers, {} a transferer vers {}",
        files.len(),
        human(total),
        addrs.join(", ")
    ));
    if opts.dry_run {
        for f in &files {
            rep.log(format!("  {} ({})", f.rel_path, human(f.size)));
        }
        rep.log(format!(
            "dry-run : {} fichier(s), {} — aucun envoi effectue",
            files.len(),
            human(total)
        ));
        return Ok(());
    }
    if opts.compress && !cfg!(feature = "compress") {
        return Err(
            "--compress necessite de compiler avec la feature `compress` (cargo build --release --features compress)"
                .into(),
        );
    }
    let manifest = Manifest {
        root_name,
        files,
        flags: if opts.compress { FLAG_COMPRESSED } else { 0 },
    };

    let sent = Arc::new(AtomicU64::new(0));
    let start = Instant::now();
    if addrs.len() == 1 {
        return send_one(
            dir, &manifest, &addrs[0], code, opts, rep, &sent, total, &start,
        );
    }
    let total_all = total.saturating_mul(addrs.len() as u64);
    let results: Vec<Result<(), String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = addrs
            .iter()
            .map(|a| {
                scope.spawn(|| {
                    send_one(dir, &manifest, a, code, opts, rep, &sent, total_all, &start)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let errors: Vec<String> = results.into_iter().filter_map(|r| r.err()).collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join(" | "))
    }
}

/// Verifie qu'une reception existante correspond a un repertoire source.
pub fn verify(src: &Path, dest_parent: &Path, rep: &dyn Reporter) -> Result<(), String> {
    if !src.is_dir() {
        return Err(format!("{} n'est pas un repertoire", src.display()));
    }
    let root_name = match src.file_name() {
        Some(n) => n.to_string_lossy().to_string(),
        None => return Err(format!("nom de racine introuvable pour {}", src.display())),
    };
    rep.log(format!("analyse de {} ...", src.display()));
    let (files, _) = scan_dir(src, &[], |_, _, _| {})?;
    let dest_root = dest_parent.join(&root_name);
    let mut ok = 0usize;
    let mut bad: Vec<String> = Vec::new();
    for f in &files {
        let p = dest_root.join(rel_to_os(&f.rel_path));
        let good = p.is_file()
            && std::fs::metadata(&p)
                .map(|m| m.len() == f.size)
                .unwrap_or(false)
            && file_hash(&p).map(|h| h == f.hash).unwrap_or(false);
        if good {
            ok += 1;
        } else {
            bad.push(f.rel_path.clone());
        }
    }
    rep.log(format!(
        "verification de {} : {}/{} fichiers conformes",
        dest_root.display(),
        ok,
        files.len()
    ));
    if bad.is_empty() {
        Ok(())
    } else {
        let head: Vec<&str> = bad.iter().take(20).map(|s| s.as_str()).collect();
        Err(format!(
            "{} fichier(s) non conformes : {}",
            bad.len(),
            head.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const TEST_CODE: &str = "AB2CD-34567";

    struct TestReporter {
        logs: Mutex<Vec<String>>,
    }
    impl TestReporter {
        fn new() -> Self {
            TestReporter {
                logs: Mutex::new(Vec::new()),
            }
        }
        fn lines(&self) -> Vec<String> {
            self.logs.lock().unwrap().clone()
        }
    }
    impl Reporter for TestReporter {
        fn log(&self, msg: String) {
            self.logs.lock().unwrap().push(msg);
        }
        fn progress(&self, _s: u64, _t: u64, _sp: f64, _p: u64) {}
        fn done(&self, _ok: bool, _n: usize, _tot: usize) {}
    }

    fn free_port() -> u16 {
        TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn make_src(name: &str) -> (std::path::PathBuf, Vec<u8>) {
        let src = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&src);
        std::fs::create_dir_all(src.join("sous/dossier")).unwrap();
        let big: Vec<u8> = (0..3 * 1024 * 1024 + 12345)
            .map(|i| (i % 251) as u8)
            .collect();
        std::fs::write(src.join("gros.bin"), &big).unwrap();
        std::fs::write(src.join("petit.txt"), b"bonjour dirtransfert").unwrap();
        std::fs::write(src.join("sous/dossier/vide.txt"), b"").unwrap();
        (src, big)
    }

    #[test]
    fn e2e_envoi_reception_complete() {
        let (src, big) = make_src("dtx_e2e_src");
        let out = std::env::temp_dir().join("dtx_e2e_out");
        let _ = std::fs::remove_dir_all(&out);
        let port = free_port();
        let addr = format!("127.0.0.1:{}", port);
        let addr2 = addr.clone();
        let out2 = out.clone();
        let rx = std::thread::spawn(move || {
            let rep = TestReporter::new();
            let r = receive(&addr2, &out2, TEST_CODE, &Opts::default(), &rep);
            (r, rep.lines())
        });
        std::thread::sleep(Duration::from_millis(150));
        let tx_rep = TestReporter::new();
        send(&src, &[addr], TEST_CODE, &Opts::default(), &tx_rep).unwrap();
        let (r, logs) = rx.join().unwrap();
        assert!(r.is_ok(), "receive : {:?}", r.err());
        assert!(logs.iter().any(|l| l.contains("transfert integre")));

        let got_big = std::fs::read(out.join("dtx_e2e_src/gros.bin")).unwrap();
        assert_eq!(got_big, big);
        assert_eq!(
            std::fs::read(out.join("dtx_e2e_src/petit.txt")).unwrap(),
            b"bonjour dirtransfert"
        );
        assert_eq!(
            std::fs::read(out.join("dtx_e2e_src/sous/dossier/vide.txt")).unwrap(),
            b""
        );
        assert!(!out.join("dtx_e2e_src/gros.bin.dtxpart").exists());

        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn e2e_reprise_par_morceaux() {
        let (src, big) = make_src("dtx_resume_src");
        let out = std::env::temp_dir().join("dtx_resume_out");
        let _ = std::fs::remove_dir_all(&out);
        // Pre-remplit un .dtxpart avec le premier morceau valide (1 Mo).
        let (files, _) = scan_dir(&src, &[], |_, _, _| {}).unwrap();
        let entry = files.iter().find(|f| f.rel_path == "gros.bin").unwrap();
        let dest_root = out.join("dtx_resume_src");
        std::fs::create_dir_all(&dest_root).unwrap();
        std::fs::write(dest_root.join("gros.bin.dtxpart"), &big[..CHUNK_SIZE]).unwrap();
        assert_eq!(entry.chunks.len(), 4);

        let port = free_port();
        let addr = format!("127.0.0.1:{}", port);
        let addr2 = addr.clone();
        let out2 = out.clone();
        let rx = std::thread::spawn(move || {
            let rep = TestReporter::new();
            let r = receive(&addr2, &out2, TEST_CODE, &Opts::default(), &rep);
            (r, rep.lines())
        });
        std::thread::sleep(Duration::from_millis(150));
        send(&src, &[addr], TEST_CODE, &Opts::default(), &NullReporter).unwrap();
        let (r, logs) = rx.join().unwrap();
        assert!(r.is_ok(), "receive : {:?}", r.err());
        assert!(
            logs.iter().any(|l| l.contains("reprise par morceaux")),
            "logs: {:?}",
            logs
        );
        assert_eq!(
            std::fs::read(out.join("dtx_resume_src/gros.bin")).unwrap(),
            big
        );

        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn e2e_manifeste_refuse() {
        let (src, _big) = make_src("dtx_refuse_src");
        let out = std::env::temp_dir().join("dtx_refuse_out");
        let _ = std::fs::remove_dir_all(&out);
        let port = free_port();
        let addr = format!("127.0.0.1:{}", port);
        let addr2 = addr.clone();
        let out2 = out.clone();
        struct RefuseReporter;
        impl Reporter for RefuseReporter {
            fn log(&self, _msg: String) {}
            fn confirm_manifest(&self, _r: &str, _f: &[(String, u64)], _t: u64) -> bool {
                false
            }
        }
        let rx = std::thread::spawn(move || {
            receive(&addr2, &out2, TEST_CODE, &Opts::default(), &RefuseReporter)
        });
        std::thread::sleep(Duration::from_millis(150));
        let res = send(&src, &[addr], TEST_CODE, &Opts::default(), &NullReporter);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("refuse"));
        assert!(rx.join().unwrap().is_err());

        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn requete_roundtrip_et_tronquee() {
        let r = encode_request("sous/fichier.bin", 7);
        let (rel, sc) = decode_request(&r).unwrap();
        assert_eq!(rel, "sous/fichier.bin");
        assert_eq!(sc, 7);
        let r0 = encode_request("a", 0);
        assert_eq!(decode_request(&r0).unwrap(), ("a".to_string(), 0));
        assert!(decode_request(&r[..10]).is_err());
        assert!(decode_request(&[]).is_err());
        let mut bad = encode_request("a", 3);
        bad[0] = 9;
        assert!(decode_request(&bad).is_err());
    }

    #[test]
    fn resume_start_chunk_detecte_prefixe_valide() {
        let data: Vec<u8> = (0..2 * CHUNK_SIZE + 100).map(|i| (i % 13) as u8).collect();
        let entry = FileEntry {
            rel_path: "f".into(),
            size: data.len() as u64,
            hash: Sha256::digest(&data).into(),
            chunks: vec![
                Sha256::digest(&data[..CHUNK_SIZE]).into(),
                Sha256::digest(&data[CHUNK_SIZE..2 * CHUNK_SIZE]).into(),
                Sha256::digest(&data[2 * CHUNK_SIZE..]).into(),
            ],
        };
        let tmp = std::env::temp_dir().join("dtx_resume_probe.dtxpart");
        std::fs::write(&tmp, &data[..CHUNK_SIZE + 50]).unwrap();
        assert_eq!(resume_start_chunk(&entry, &tmp), 1);
        std::fs::write(&tmp, &data[..CHUNK_SIZE]).unwrap();
        assert_eq!(resume_start_chunk(&entry, &tmp), 1);
        std::fs::write(&tmp, b"xxxx").unwrap();
        assert_eq!(resume_start_chunk(&entry, &tmp), 0);
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(resume_start_chunk(&entry, &tmp), 0);
    }

    #[test]
    fn receive_file_rejette_morceau_invalide() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            let mut ss = SecureStream::server_handshake(sock, TEST_CODE).unwrap();
            write_kind(&mut ss, KIND_CHUNK, b"donnees corrompues").unwrap();
            write_kind(&mut ss, KIND_META, b"").unwrap();
        });
        let c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut cs = SecureStream::client_handshake(c, TEST_CODE).unwrap();
        let entry = FileEntry {
            rel_path: "f.bin".into(),
            size: 19,
            hash: [9u8; 32],
            chunks: vec![[8u8; 32]],
        };
        let dest = std::env::temp_dir().join("dtx_bad_chunk.bin");
        let sent = AtomicU64::new(0);
        let res = receive_file(
            &mut cs,
            &dest,
            &entry,
            0,
            false,
            &None,
            &NullReporter,
            &sent,
            19,
            &Instant::now(),
        );
        assert!(res.is_err());
        assert!(!dest.exists());
        server.join().unwrap();
        let _ = std::fs::remove_file(part_path(&dest));
    }

    #[test]
    fn verify_detecte_conforme_et_non_conforme() {
        let (src, _big) = make_src("dtx_verify_src");
        let out = std::env::temp_dir().join("dtx_verify_out");
        let _ = std::fs::remove_dir_all(&out);
        let dest_root = out.join("dtx_verify_src");
        std::fs::create_dir_all(dest_root.join("sous/dossier")).unwrap();
        std::fs::copy(src.join("gros.bin"), dest_root.join("gros.bin")).unwrap();
        std::fs::copy(src.join("petit.txt"), dest_root.join("petit.txt")).unwrap();
        std::fs::write(dest_root.join("sous/dossier/vide.txt"), b"").unwrap();
        let rep = TestReporter::new();
        assert!(verify(&src, &out, &rep).is_ok());
        std::fs::write(dest_root.join("petit.txt"), b"corrompu").unwrap();
        assert!(verify(&src, &out, &rep).is_err());
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn dry_run_n_envoie_rien() {
        let (src, _big) = make_src("dtx_dry_src");
        let rep = TestReporter::new();
        let opts = Opts {
            dry_run: true,
            ..Default::default()
        };
        send(&src, &["127.0.0.1:1".to_string()], TEST_CODE, &opts, &rep).unwrap();
        assert!(rep.lines().iter().any(|l| l.contains("dry-run")));
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn exclude_filtre_les_fichiers() {
        let src = std::env::temp_dir().join("dtx_excl_src");
        let _ = std::fs::remove_dir_all(&src);
        std::fs::create_dir_all(src.join("node_modules/pkg")).unwrap();
        std::fs::write(src.join("keep.txt"), b"x").unwrap();
        std::fs::write(src.join("node_modules/pkg/index.js"), b"y").unwrap();
        std::fs::write(src.join("temp.tmp"), b"z").unwrap();
        let rep = TestReporter::new();
        let opts = Opts {
            dry_run: true,
            exclude: vec!["node_modules".to_string(), "*.tmp".to_string()],
            ..Default::default()
        };
        send(&src, &["127.0.0.1:1".to_string()], TEST_CODE, &opts, &rep).unwrap();
        let logs = rep.lines();
        assert!(logs.iter().any(|l| l.contains("keep.txt")));
        assert!(!logs.iter().any(|l| l.contains("index.js")));
        assert!(!logs.iter().any(|l| l.contains("temp.tmp")));
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn send_racine_sans_nom_erreur_propre() {
        let res = send(
            Path::new("/"),
            &["127.0.0.1:1".to_string()],
            TEST_CODE,
            &Opts::default(),
            &NullReporter,
        );
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("racine"));
    }

    #[test]
    fn annulation_arrete_l_envoi() {
        let dir = std::env::temp_dir().join("dtx_cancel_src");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"data").unwrap();

        let cancel = Arc::new(AtomicBool::new(true));
        let opts = Opts {
            cancel: Some(cancel),
            ..Default::default()
        };
        let res = send(
            &dir,
            &["127.0.0.1:1".to_string()],
            TEST_CODE,
            &opts,
            &NullReporter,
        );
        assert_eq!(res.unwrap_err(), CANCELLED_MSG);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fenetre_pipeline_bornees() {
        let o = Opts {
            pipeline: Some(0),
            ..Default::default()
        };
        assert_eq!(o.pipeline.unwrap_or(8).clamp(1, 64), 1);
        let o = Opts {
            pipeline: Some(999),
            ..Default::default()
        };
        assert_eq!(o.pipeline.unwrap_or(8).clamp(1, 64), 64);
    }

    #[test]
    fn e2e_ipv6_si_disponible() {
        if TcpListener::bind("[::1]:0").is_err() {
            return; // pas de loopback IPv6 dans cet environnement
        }
        let src = std::env::temp_dir().join("dtx_e2e_ipv6_src");
        let out = std::env::temp_dir().join("dtx_e2e_ipv6_out");
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("test_ipv6.txt"), b"test ipv6 content").unwrap();

        let port = TcpListener::bind("[::1]:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let addr = format!("[::1]:{}", port);
        let addr2 = addr.clone();
        let out2 = out.clone();
        let rx = std::thread::spawn(move || {
            receive(&addr2, &out2, TEST_CODE, &Opts::default(), &NullReporter)
        });
        std::thread::sleep(Duration::from_millis(150));
        send(&src, &[addr], TEST_CODE, &Opts::default(), &NullReporter).unwrap();
        assert!(rx.join().unwrap().is_ok());
        assert_eq!(
            std::fs::read(out.join("dtx_e2e_ipv6_src/test_ipv6.txt")).unwrap(),
            b"test ipv6 content"
        );
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&out);
    }
}

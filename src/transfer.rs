use crate::crypto::SecureStream;
use crate::firewall::RULE_NAME;
use crate::protocol::*;
use crate::{firewall, upnp};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const PIPELINE_WINDOW: usize = 8;
const IO_TIMEOUT: Duration = Duration::from_secs(30);

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

fn write_stream_atomically<F>(
    dest: &Path,
    expected_size: u64,
    expected_hash: [u8; 32],
    mut chunks: F,
) -> Result<(), String>
where
    F: FnMut(&mut dyn FnMut(&[u8]) -> Result<(), String>) -> Result<(), String>,
{
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creation de {} : {}", parent.display(), e))?;
    }
    let tmp = dest.with_extension("dtxpart");
    let mut f = std::fs::File::create(&tmp)
        .map_err(|e| format!("creation de {} : {}", tmp.display(), e))?;
    let mut hasher = Sha256::new();
    let mut total: u64 = 0;
    chunks(&mut |data| {
        f.write_all(data)
            .map_err(|e| format!("ecriture de {} : {}", tmp.display(), e))?;
        hasher.update(data);
        total = total.wrapping_add(data.len() as u64);
        Ok(())
    })?;
    if total != expected_size {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "taille invalide pour {} ({} octets attendus)",
            dest.display(),
            expected_size
        ));
    }
    let got: [u8; 32] = hasher.finalize().into();
    if got != expected_hash {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("checksum invalide pour {}", dest.display()));
    }
    f.sync_all()
        .map_err(|e| format!("sync de {} : {}", tmp.display(), e))?;
    std::fs::rename(&tmp, dest)
        .map_err(|e| format!("renommage vers {} : {}", dest.display(), e))?;
    Ok(())
}

fn write_empty_atomically(dest: &Path, expected_hash: [u8; 32]) -> Result<(), String> {
    write_stream_atomically(dest, 0, expected_hash, |emit| emit(&[]))
}

fn load_existing_hashes(
    dest_root: &Path,
    manifest: &Manifest,
) -> std::collections::HashMap<String, [u8; 32]> {
    let mut done = std::collections::HashMap::new();
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
    let (host, port) = s.rsplit_once(':')?;
    if host.is_empty() {
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
    pub cancel: Option<Arc<AtomicBool>>,
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
    fn progress(&self, sent: u64, total: u64, speed_mbps: f64, pct: u64);
    fn done(&self, ok: bool, files_ok: usize, files_total: usize);
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
    if opts.upnp {
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
                    "sur l'emetteur, lancez : DirTransfert send <repertoire> {}:{}",
                    ext, m.external_port
                ));
                upnp_map = Some(m);
            }
            Err(e) => rep.log(format!(
                "UPnP : {} (configurez la redirection manuellement sur la box)",
                e
            )),
        }
    }
    let ip_locale = upnp::local_ip();
    if !ip_locale.is_empty() {
        rep.log(format!("IP locale de ce PC : {}", ip_locale));
        if let Ok(l) = crate::link::encode(&ip_locale, port, code) {
            rep.log(format!("code de connexion (meme reseau) : {}", l));
        }
    }
    match upnp::get_external_ip() {
        Ok(ip_pub) => {
            rep.log(format!("IP publique de la box : {}", ip_pub));
            if let Ok(l) = crate::link::encode(&ip_pub, port, code) {
                rep.log(format!("code de connexion (via Internet) : {}", l));
            }
            if ip_pub.starts_with("100.") && !ip_pub.starts_with("100.128.") {
                rep.log("attention : IP en plage 100.64-127.x.x = CGNAT operateur, l'UPnP ne suffira pas, utilisez un VPN (Tailscale/WireGuard)".to_string());
            }
        }
        Err(e) => {
            rep.log(format!("IP publique : {} (via Internet, trouvez l'IP de la box sur son interface d'administration)", e));
        }
    }
    let listener = TcpListener::bind(addr).map_err(|e| format!("liaison de {} : {}", addr, e))?;
    rep.log(format!("en ecoute sur {}", addr));
    let (sock, peer) = listener
        .accept()
        .map_err(|e| format!("acceptation : {}", e))?;
    rep.log(format!("connexion de {}", peer));
    let mut stream = SecureStream::server_handshake(sock, code)
        .map_err(|e| format!("poignee de main chiffree avec {} : {}", peer, e))?;
    stream.set_timeouts(Some(IO_TIMEOUT), Some(IO_TIMEOUT));
    rep.log("flux chiffre (ChaCha20-Poly1305, code de jumelage verifie)".to_string());

    let msg = read_msg(&mut stream).map_err(|e| format!("reception du manifeste : {}", e))?;
    let manifest = Manifest::decode(&msg)?;
    let total: u64 = manifest.files.iter().map(|f| f.size).sum();
    rep.log(format!(
        "transfert de \"{}\" : {} fichiers, {}",
        manifest.root_name,
        manifest.files.len(),
        human(total)
    ));

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
    let mut verified: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut failures: Vec<String> = Vec::new();

    let to_request: Vec<&FileEntry> = manifest
        .files
        .iter()
        .filter(|entry| {
            if entry.size == 0 {
                return false;
            }
            if let Some(h) = existing.get(&entry.rel_path) {
                if *h == entry.hash {
                    sent.fetch_add(entry.size, Ordering::Relaxed);
                    verified.insert(entry.rel_path.clone());
                    return false;
                }
            }
            true
        })
        .collect();

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
    let mut pending: std::collections::VecDeque<&FileEntry> = to_request.iter().copied().collect();
    let mut in_flight: std::collections::VecDeque<&FileEntry> = std::collections::VecDeque::new();

    while !pending.is_empty() && in_flight.len() < window {
        let entry = pending.pop_front().unwrap();
        write_msg(&mut stream, entry.rel_path.as_bytes())
            .map_err(|e| format!("envoi de requete pour {} : {}", entry.rel_path, e))?;
        in_flight.push_back(entry);
    }

    while let Some(entry) = in_flight.pop_front() {
        if cancelled(&opts.cancel) {
            break;
        }
        let dest = dest_root.join(rel_to_os(&entry.rel_path));
        let res = write_stream_atomically(&dest, entry.size, entry.hash, |emit| loop {
            check_cancel(&opts.cancel)?;
            let (kind, data) = read_kind(&mut stream)
                .map_err(|e| format!("reception de {} : {}", entry.rel_path, e))?;
            match kind {
                KIND_CHUNK => {
                    if data.is_empty() {
                        return Err(format!("{} : morceau vide du recepteur", entry.rel_path));
                    }
                    emit(&data)?;
                    sent.fetch_add(data.len() as u64, Ordering::Relaxed);
                    progress_report(rep, &sent, total, &start);
                }
                KIND_META if data.is_empty() => return Ok(()),
                _ => return Err(format!("{} : type de trame inattendu", entry.rel_path)),
            }
        });
        if let Err(e) = res {
            failures.push(format!("{} : {}", entry.rel_path, e));
        } else {
            verified.insert(entry.rel_path.clone());
        }
        if !pending.is_empty() {
            let next = pending.pop_front().unwrap();
            write_msg(&mut stream, next.rel_path.as_bytes())
                .map_err(|e| format!("envoi de requete pour {} : {}", next.rel_path, e))?;
            in_flight.push_back(next);
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

pub fn send(
    dir: &Path,
    addr: &str,
    code: &str,
    cancel: &Option<Arc<AtomicBool>>,
    rep: &dyn Reporter,
) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("{} n'est pas un repertoire", dir.display()));
    }
    check_cancel(cancel)?;
    let root_name = dir.file_name().unwrap().to_string_lossy().to_string();
    rep.log(format!("analyse de {} ...", dir.display()));
    let last_report = Arc::new(std::sync::Mutex::new((0usize, std::time::Instant::now())));
    let scan_rep = rep;
    let scan_progress = move |done: usize, total: usize, current: &str| {
        let Ok(mut last) = last_report.lock() else {
            return;
        };
        let advance = done > last.0;
        let elapsed = last.1.elapsed().as_millis() as usize;
        if done == total || (advance && elapsed >= 200) {
            *last = (done, std::time::Instant::now());
            drop(last);
            scan_rep.scan(done, total, current);
        }
    };
    let files = scan_dir(dir, scan_progress)?;
    let total: u64 = files.iter().map(|f| f.size).sum();
    rep.scan(files.len(), files.len(), "analyse terminee");
    rep.log(format!(
        "{} fichiers, {} a transferer vers {}",
        files.len(),
        human(total),
        addr
    ));

    let sock = TcpStream::connect(addr).map_err(|e| format!("connexion a {} : {}", addr, e))?;
    sock.set_nodelay(true).ok();
    let mut stream = SecureStream::client_handshake(sock, code)
        .map_err(|e| format!("poignee de main chiffree avec {} : {}", addr, e))?;
    stream.set_timeouts(Some(IO_TIMEOUT), Some(IO_TIMEOUT));
    rep.log("flux chiffre (ChaCha20-Poly1305, code de jumelage verifie)".to_string());

    let manifest = Manifest { root_name, files };
    write_msg(&mut stream, &manifest.encode())
        .map_err(|e| format!("envoi du manifeste : {}", e))?;

    let sent = Arc::new(AtomicU64::new(0));
    let start = Instant::now();

    let mut remaining = manifest.files.iter().filter(|f| f.size > 0).count();
    while remaining > 0 {
        check_cancel(cancel)?;
        let data = read_msg(&mut stream).map_err(|e| format!("protocole : {}", e))?;
        if data.is_empty() {
            return Err("requete vide du recepteur".into());
        }
        let rel = String::from_utf8_lossy(&data).to_string();
        let ep = dir.join(rel_to_os(&rel));
        let mut f = std::fs::File::open(&ep)
            .map_err(|e| format!("ouverture de {} : {}", ep.display(), e))?;
        let mut buf = vec![0u8; CHUNK_SIZE];
        loop {
            let n = f
                .read(&mut buf)
                .map_err(|e| format!("lecture de {} : {}", ep.display(), e))?;
            if n == 0 {
                break;
            }
            write_kind(&mut stream, KIND_CHUNK, &buf[..n])
                .map_err(|e| format!("envoi de {} : {}", rel, e))?;
            sent.fetch_add(n as u64, Ordering::Relaxed);
            progress_report(rep, &sent, total, &start);
        }
        write_kind(&mut stream, KIND_META, b"").map_err(|e| format!("fin de {} : {}", rel, e))?;
        remaining -= 1;
    }

    let fin = read_msg(&mut stream).map_err(|e| format!("protocole : {}", e))?;
    if fin == b"OK" {
        rep.log("recepteur confirme l'integrite".to_string());
        rep.done(true, manifest.files.len(), manifest.files.len());
        return Ok(());
    }
    if fin == b"FAIL" {
        return Err("recepteur signale des checksums invalides (voir son journal)".into());
    }
    Err(format!(
        "statut final inattendu du recepteur : {} octets",
        fin.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfert_e2e_multi_fichiers_reprise() {
        let src_dir = std::env::temp_dir().join("dtx_e2e_src");
        let out_dir = std::env::temp_dir().join("dtx_e2e_out");
        let _ = std::fs::remove_dir_all(&src_dir);
        let _ = std::fs::remove_dir_all(&out_dir);
        std::fs::create_dir_all(src_dir.join("sous/dossier")).unwrap();
        let big: Vec<u8> = (0..3 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
        std::fs::write(src_dir.join("gros.bin"), &big).unwrap();
        std::fs::write(src_dir.join("petit.txt"), b"bonjour dirtransfert").unwrap();
        std::fs::write(src_dir.join("sous/dossier/vide.txt"), b"").unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            let mut stream = SecureStream::server_handshake(sock, "ABCD-2345").unwrap();
            stream.set_timeouts(Some(Duration::from_secs(30)), Some(Duration::from_secs(30)));
            drop(listener);

            let msg = read_msg(&mut stream).unwrap();
            let manifest = Manifest::decode(&msg).unwrap();
            let total: u64 = manifest.files.iter().map(|f| f.size).sum();
            assert_eq!(manifest.files.len(), 3);
            assert_eq!(total, (3 * 1024 * 1024 + 20) as u64);

            let dest_root = std::env::temp_dir()
                .join("dtx_e2e_out")
                .join(&manifest.root_name);
            std::fs::create_dir_all(&dest_root).unwrap();
            for entry in &manifest.files {
                if entry.size == 0 {
                    write_empty_atomically(&dest_root.join(rel_to_os(&entry.rel_path)), entry.hash)
                        .unwrap();
                    continue;
                }
                write_msg(&mut stream, entry.rel_path.as_bytes()).unwrap();
                write_stream_atomically(
                    &dest_root.join(rel_to_os(&entry.rel_path)),
                    entry.size,
                    entry.hash,
                    |emit| loop {
                        let (kind, data) = read_kind(&mut stream).map_err(|e| e.to_string())?;
                        match kind {
                            KIND_CHUNK => emit(&data)?,
                            KIND_META if data.is_empty() => return Ok(()),
                            _ => return Err("trame inattendue".into()),
                        }
                    },
                )
                .unwrap();
            }
            write_msg(&mut stream, b"OK").unwrap();
        });

        let rep = NullReporter;
        send(
            &src_dir,
            &format!("127.0.0.1:{}", port),
            "ABCD-2345",
            &None,
            &rep,
        )
        .unwrap();
        server.join().unwrap();

        let got_big = std::fs::read(out_dir.join("dtx_e2e_src/gros.bin")).unwrap();
        assert_eq!(got_big.len(), big.len());
        assert_eq!(
            file_hash(Path::new(&out_dir.join("dtx_e2e_src/gros.bin"))).unwrap(),
            file_hash(Path::new(&src_dir.join("gros.bin"))).unwrap()
        );
        assert_eq!(
            std::fs::read(out_dir.join("dtx_e2e_src/petit.txt")).unwrap(),
            b"bonjour dirtransfert"
        );

        let _ = std::fs::remove_dir_all(&src_dir);
        let _ = std::fs::remove_dir_all(&out_dir);
    }

    #[test]
    fn ecriture_atomique_rejette_mauvais_hash() {
        let dest = std::env::temp_dir().join("dtx_bad_hash.txt");
        let bad = [0u8; 32];
        let res = write_stream_atomically(&dest, 3, bad, |emit| emit(b"abc"));
        assert!(res.is_err());
        assert!(!dest.exists());
    }
}

#[cfg(test)]
mod cancel_tests {
    use super::*;

    #[test]
    fn annulation_arrete_l_envoi() {
        let dir = std::env::temp_dir().join("dtx_cancel_src");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"data").unwrap();

        let cancel = Arc::new(AtomicBool::new(true));
        let rep = NullReporter;
        let res = send(&dir, "127.0.0.1:1", "ABCD-2345", &Some(cancel), &rep);
        assert!(res.is_err());
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
}

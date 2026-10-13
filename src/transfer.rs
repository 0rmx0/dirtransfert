use crate::crypto::SecureStream;
use crate::firewall::RULE_NAME;
use crate::protocol::*;
use crate::sha256::Sha256;
use crate::{firewall, upnp};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

pub fn human(bytes: u64) -> String {
    let units = ["o", "Ko", "Mo", "Go", "To"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < 4 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{} {}", bytes, units[i]) } else { format!("{:.2} {}", v, units[i]) }
}

fn read_file_hashed(path: &Path) -> Result<([u8; 32], Vec<u8>), String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("ouverture de {} : {}", path.display(), e))?;
    let mut hasher = Sha256::new();
    let mut data = Vec::new();
    let mut buf = vec![0u8; 4 * 1024 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("lecture de {} : {}", path.display(), e))?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
        data.extend_from_slice(&buf[..n]);
    }
    Ok((hasher.finalize(), data))
}

fn write_file_atomically(dest: &Path, data: &[u8], expected_hash: [u8; 32]) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("creation de {} : {}", parent.display(), e))?;
    }
    let tmp = dest.with_extension("dtxpart");
    let mut f = std::fs::File::create(&tmp).map_err(|e| format!("creation de {} : {}", tmp.display(), e))?;
    f.write_all(data).map_err(|e| format!("ecriture de {} : {}", tmp.display(), e))?;
    f.sync_all().map_err(|e| format!("sync de {} : {}", tmp.display(), e))?;
    let mut hasher = Sha256::new();
    hasher.update(data);
    let got = hasher.finalize();
    if got != expected_hash {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("checksum invalide pour {}", dest.display()));
    }
    std::fs::rename(&tmp, dest).map_err(|e| format!("renommage vers {} : {}", dest.display(), e))?;
    Ok(())
}

fn load_existing_hashes(dest_root: &Path, manifest: &Manifest) -> std::collections::HashMap<String, [u8; 32]> {
    let mut done = std::collections::HashMap::new();
    for entry in &manifest.files {
        let p = dest_root.join(rel_to_os(&entry.rel_path));
        if p.is_file() {
            if let Ok(meta) = std::fs::metadata(&p) {
                if meta.len() == entry.size {
                    if let Ok((h, _)) = read_file_hashed(&p) {
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
    if host.is_empty() { return None; }
    port.parse::<u16>().ok().map(|p| (host.to_string(), p))
}

#[derive(Default, Clone)]
pub struct Opts {
    pub upnp: bool,
    pub open_fw: bool,
    pub close_fw_after: bool,
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

pub fn receive(addr: &str, out_dir: &Path, code: &str, opts: &Opts, rep: &dyn Reporter) -> Result<(), String> {
    let Some((_, port)) = parse_endpoint(addr) else {
        return Err(format!("adresse \"{}\" invalide (attendu hote:port)", addr));
    };
    if opts.open_fw {
        match firewall::ensure_firewall_rule(port) {
            Ok(_) => rep.log(format!("regle pare-feu \"{}\" ouverte sur le port TCP {}", RULE_NAME, port)),
            Err(e) => rep.log(format!("pare-feu : {} (ouvrez le port {} manuellement)", e, port)),
        }
    }
    let mut upnp_map = None;
    if opts.upnp {
        match upnp::add_port_mapping(port) {
            Ok(m) => {
                rep.log(format!("redirection UPnP active : port TCP {} -> ce PC", m.external_port));
                let ext = if m.external_ip.is_empty() { String::from("<IP_publique_de_la_box>") } else { m.external_ip.clone() };
                rep.log(format!("sur l'emetteur, lancez : DirTransfert send <repertoire> {}:{}", ext, m.external_port));
                upnp_map = Some(m);
            }
            Err(e) => rep.log(format!("UPnP : {} (configurez la redirection manuellement sur la box)", e)),
        }
    }
    let ip_locale = upnp::local_ip();
    if !ip_locale.is_empty() {
        rep.log(format!("IP locale de ce PC : {}", ip_locale));
    }
    match upnp::get_external_ip() {
        Ok(ip_pub) => {
            rep.log(format!("IP publique de la box : {}", ip_pub));
            rep.log(format!("sur le PC emetteur (meme reseau), renseignez : {}:{}", ip_locale, port));
            rep.log(format!("sur le PC emetteur (via Internet), renseignez : {}:{}", ip_pub, port));
            if ip_pub.starts_with("100.") && !ip_pub.starts_with("100.128.") {
                rep.log("attention : IP en plage 100.64-127.x.x = CGNAT operateur, l'UPnP ne suffira pas, utilisez un VPN (Tailscale/WireGuard)".to_string());
            }
        }
        Err(e) => {
            if !ip_locale.is_empty() {
                rep.log(format!("sur le PC emetteur (meme reseau), renseignez : {}:{}", ip_locale, port));
            }
            rep.log(format!("IP publique : {} (via Internet, trouvez l'IP de la box sur son interface d'administration)", e));
        }
    }
    let listener = TcpListener::bind(addr).map_err(|e| format!("liaison de {} : {}", addr, e))?;
    rep.log(format!("en ecoute sur {}", addr));
    let (sock, peer) = listener.accept().map_err(|e| format!("acceptation : {}", e))?;
    rep.log(format!("connexion de {}", peer));
    let mut stream = SecureStream::server_handshake(sock, code)
        .map_err(|e| format!("poignee de main chiffree avec {} : {}", peer, e))?;
    rep.log("flux chiffre (ChaCha20-Poly1305, code de jumelage verifie)".to_string());

    let msg = read_msg(&mut stream).map_err(|e| format!("reception du manifeste : {}", e))?;
    let manifest = Manifest::decode(&msg)?;
    let total: u64 = manifest.files.iter().map(|f| f.size).sum();
    rep.log(format!("transfert de \"{}\" : {} fichiers, {}", manifest.root_name, manifest.files.len(), human(total)));

    let dest_root = out_dir.join(&manifest.root_name);
    std::fs::create_dir_all(&dest_root).map_err(|e| format!("creation de {} : {}", dest_root.display(), e))?;

    let existing = load_existing_hashes(&dest_root, &manifest);
    if !existing.is_empty() {
        rep.log(format!("reprise : {} fichier(s) deja valides", existing.len()));
    }

    let sent = Arc::new(AtomicU64::new(0));
    let start = Instant::now();

    let mut failures: Vec<String> = Vec::new();
    for entry in &manifest.files {
        if let Some(h) = existing.get(&entry.rel_path) {
            if *h == entry.hash {
                sent.fetch_add(entry.size, Ordering::Relaxed);
                continue;
            }
        }
        if entry.size == 0 {
            write_file_atomically(&dest_root.join(rel_to_os(&entry.rel_path)), &[], entry.hash)?;
            sent.fetch_add(entry.size, Ordering::Relaxed);
            continue;
        }
        write_msg(&mut stream, entry.rel_path.as_bytes())
            .map_err(|e| format!("envoi de requete pour {} : {}", entry.rel_path, e))?;
        let data = read_msg(&mut stream)
            .map_err(|e| format!("reception de {} : {}", entry.rel_path, e))?;
        if let Err(e) = write_file_atomically(&dest_root.join(rel_to_os(&entry.rel_path)), &data, entry.hash) {
            failures.push(format!("{} : {}", entry.rel_path, e));
        }
        sent.fetch_add(entry.size, Ordering::Relaxed);
        let s = sent.load(Ordering::Relaxed);
        let el = start.elapsed().as_secs_f64();
        let speed = if el > 0.0 { s as f64 / el } else { 0.0 };
        let pct = if total > 0 { s * 100 / total } else { 100 };
        rep.progress(s, total, speed / 1024.0 / 1024.0, pct);
    }

    let final_hashes = load_existing_hashes(&dest_root, &manifest);
    let mut ok = 0;
    for entry in &manifest.files {
        match final_hashes.get(&entry.rel_path) {
            Some(h) if *h == entry.hash => ok += 1,
            _ => failures.push(format!("{} : checksum final invalide", entry.rel_path)),
        }
    }

    write_msg(&mut stream, b"OK").ok();
    drop(stream);

    if let Some(m) = upnp_map {
        match upnp::delete_port_mapping(m.external_port) {
            Ok(()) => rep.log(format!("redirection UPnP fermee (port {})", m.external_port)),
            Err(e) => rep.log(format!("fermeture UPnP : {} (fermez le port manuellement)", e)),
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
        return Err(format!("transfert incomplet ({}/{} ok) : {}", ok, manifest.files.len(), head.join("; ")));
    }
    rep.log("transfert integre, checksums verifies".to_string());
    Ok(())
}

pub fn send(dir: &Path, addr: &str, code: &str, rep: &dyn Reporter) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("{} n'est pas un repertoire", dir.display()));
    }
    let root_name = dir.file_name().unwrap().to_string_lossy().to_string();
    rep.log(format!("analyse de {} ...", dir.display()));
    let last_report = Arc::new(std::sync::Mutex::new((0usize, std::time::Instant::now())));
    let scan_rep = rep;
    let scan_progress = move |done: usize, total: usize, current: &str| {
        let Ok(mut last) = last_report.lock() else { return };
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
    rep.log(format!("{} fichiers, {} a transferer vers {}", files.len(), human(total), addr));

    let sock = TcpStream::connect(addr).map_err(|e| format!("connexion a {} : {}", addr, e))?;
    sock.set_nodelay(true).ok();
    let mut stream = SecureStream::client_handshake(sock, code)
        .map_err(|e| format!("poignee de main chiffree avec {} : {}", addr, e))?;
    rep.log("flux chiffre (ChaCha20-Poly1305, code de jumelage verifie)".to_string());

    let manifest = Manifest { root_name, files };
    write_msg(&mut stream, &manifest.encode())
        .map_err(|e| format!("envoi du manifeste : {}", e))?;

    let sent = Arc::new(AtomicU64::new(0));
    let start = Instant::now();

    let mut sent_files = 0;
    for entry in &manifest.files {
        if entry.size == 0 {
            sent_files += 1;
            continue;
        }
        loop {
            let data = read_msg(&mut stream).map_err(|e| format!("protocole : {}", e))?;
            if data.is_empty() {
                return Err("requete vide du recepteur".into());
            }
            if data == b"OK" {
                rep.log("recepteur confirme l'integrite".to_string());
                rep.done(true, sent_files, manifest.files.len());
                return Ok(());
            }
            let rel = String::from_utf8_lossy(&data).to_string();
            let ep = dir.join(rel_to_os(&rel));
            match std::fs::File::open(&ep) {
                Ok(mut f) => {
                    let mut buf = Vec::new();
                    f.read_to_end(&mut buf).map_err(|e| format!("lecture de {} : {}", ep.display(), e))?;
                    write_msg(&mut stream, &buf).map_err(|e| format!("envoi de {} : {}", rel, e))?;
                    sent_files += 1;
                    sent.fetch_add(buf.len() as u64, Ordering::Relaxed);
                    let s = sent.load(Ordering::Relaxed);
                    let el = start.elapsed().as_secs_f64();
                    let speed = if el > 0.0 { s as f64 / el } else { 0.0 };
                    let pct = if total > 0 { s * 100 / total } else { 100 };
                    rep.progress(s, total, speed / 1024.0 / 1024.0, pct);
                    break;
                }
                Err(e) => {
                    write_msg(&mut stream, b"").ok();
                    return Err(format!("ouverture de {} : {}", ep.display(), e));
                }
            }
        }
    }

    if manifest.files.iter().all(|f| f.size == 0) {
        write_msg(&mut stream, b"").ok();
        let fin = read_msg(&mut stream).map_err(|e| format!("protocole : {}", e))?;
        if fin == b"OK" {
            rep.log("recepteur confirme l'integrite".to_string());
            rep.done(true, manifest.files.len(), manifest.files.len());
            return Ok(());
        }
    }

    rep.log(format!("{} fichiers envoyes, attente de la confirmation finale", sent_files));
    Ok(())
}

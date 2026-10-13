mod firewall;
mod protocol;
mod sha256;
mod upnp;

use firewall::RULE_NAME;
use protocol::*;
use sha256::Sha256;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

const DEFAULT_PORT: u16 = 47500;

fn human(bytes: u64) -> String {
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

fn parse_endpoint(s: &str) -> Option<(String, u16)> {
    let (host, port) = s.rsplit_once(':')?;
    if host.is_empty() { return None; }
    port.parse::<u16>().ok().map(|p| (host.to_string(), p))
}

fn cmd_receive(addr: &str, out_dir: &Path, opts: &Opts) -> Result<(), String> {
    let Some((_, port)) = parse_endpoint(addr) else {
        return Err(format!("adresse \"{}\" invalide (attendu hote:port)", addr));
    };
    if opts.open_fw {
        match firewall::ensure_firewall_rule(port) {
            Ok(_) => println!("[recv] regle pare-feu \"{}\" ouverte sur le port TCP {}", RULE_NAME, port),
            Err(e) => eprintln!("[recv] pare-feu : {} (ouvrez le port {} manuellement)", e, port),
        }
    }
    let mut upnp_map = None;
    if opts.upnp {
        match upnp::add_port_mapping(port) {
            Ok(m) => {
                println!("[recv] redirection UPnP active : port TCP {} -> ce PC", m.external_port);
                let ext = if m.external_ip.is_empty() { String::from("<IP_publique_de_la_box>") } else { m.external_ip.clone() };
                println!("[recv] sur l'emetteur, lancez : dirtransfer send <repertoire> {}:{}", ext, m.external_port);
                upnp_map = Some(m);
            }
            Err(e) => eprintln!("[recv] UPnP : {} (configurez la redirection manuellement sur la box)", e),
        }
    }
    let listener = TcpListener::bind(addr).map_err(|e| format!("liaison de {} : {}", addr, e))?;
    println!("[recv] en ecoute sur {}", addr);
    let (mut stream, peer) = listener.accept().map_err(|e| format!("acceptation : {}", e))?;
    println!("[recv] connexion de {}", peer);

    let msg = read_msg(&mut stream).map_err(|e| format!("reception du manifeste : {}", e))?;
    let manifest = Manifest::decode(&msg)?;
    let total: u64 = manifest.files.iter().map(|f| f.size).sum();
    println!(
        "[recv] transfert de \"{}\" : {} fichiers, {}",
        manifest.root_name,
        manifest.files.len(),
        human(total)
    );

    let dest_root = out_dir.join(&manifest.root_name);
    std::fs::create_dir_all(&dest_root).map_err(|e| format!("creation de {} : {}", dest_root.display(), e))?;

    let existing = load_existing_hashes(&dest_root, &manifest);
    if !existing.is_empty() {
        println!("[recv] reprise : {} fichier(s) deja valides", existing.len());
    }

    let sent = Arc::new(AtomicU64::new(0));
    let running = Arc::new(AtomicBool::new(true));
    let start = Instant::now();
    let tick = {
        let sent = sent.clone();
        let running = running.clone();
        let total = total;
        std::thread::spawn(move || {
            while running.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_secs(1));
                if !running.load(Ordering::Relaxed) { break; }
                let s = sent.load(Ordering::Relaxed);
                let el = start.elapsed().as_secs_f64();
                let speed = if el > 0.0 { s as f64 / el } else { 0.0 };
                let pct = if total > 0 { s * 100 / total } else { 100 };
                print!("\r[recv] {} / {} ({:.0} Mo/s) {}%   ", human(s), human(total), speed / 1024.0 / 1024.0, pct);
                let _ = std::io::stdout().flush();
            }
        })
    };

    let mut failures = Vec::new();
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
        write_file_atomically(&dest_root.join(rel_to_os(&entry.rel_path)), &data, entry.hash)
            .map_err(|e| {
                failures.push(format!("{} : {}", entry.rel_path, e));
                String::new()
            })
            .ok();
        sent.fetch_add(entry.size, Ordering::Relaxed);
    }
    running.store(false, Ordering::Relaxed);
    let _ = tick.join();
    println!();

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
            Ok(()) => println!("[recv] redirection UPnP fermee (port {})", m.external_port),
            Err(e) => eprintln!("[recv] fermeture UPnP : {} (fermez le port manuellement)", e),
        }
    }
    if opts.close_fw_after {
        match firewall::remove_firewall_rule() {
            Ok(()) => println!("[recv] regle pare-feu \"{}\" supprimee", RULE_NAME),
            Err(e) => eprintln!("[recv] suppression regle pare-feu : {}", e),
        }
    }

    println!("[recv] termines : {}/{} fichiers valides", ok, manifest.files.len());
    if !failures.is_empty() {
        eprintln!("[recv] echecs ({}) :", failures.len());
        for f in failures.iter().take(20) {
            eprintln!("  - {}", f);
        }
        return Err("transfert incomplet".into());
    }
    println!("[recv] transfert integre, checksums verifies");
    Ok(())
}

fn cmd_send(dir: &Path, addr: &str) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("{} n'est pas un repertoire", dir.display()));
    }
    let root_name = dir.file_name().unwrap().to_string_lossy().to_string();
    println!("[send] analyse de {} ...", dir.display());
    let files = scan_dir(dir)?;
    let total: u64 = files.iter().map(|f| f.size).sum();
    println!("[send] {} fichiers, {} a transferer vers {}", files.len(), human(total), addr);

    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connexion a {} : {}", addr, e))?;
    stream.set_nodelay(true).ok();

    let manifest = Manifest { root_name, files };
    write_msg(&mut stream, &manifest.encode())
        .map_err(|e| format!("envoi du manifeste : {}", e))?;

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
                println!("[send] recepteur confirme l'integrite");
                return Ok(());
            }
            let rel = String::from_utf8_lossy(&data).to_string();
            let ep = dir.join(rel_to_os(&rel));
            match std::fs::File::open(&ep) {
                Ok(mut f) => {
                    let mut buf = Vec::with_capacity(entry.size as usize);
                    f.read_to_end(&mut buf).map_err(|e| format!("lecture de {} : {}", ep.display(), e))?;
                    write_msg(&mut stream, &buf).map_err(|e| format!("envoi de {} : {}", rel, e))?;
                    sent_files += 1;
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
            println!("[send] recepteur confirme l'integrite");
            return Ok(());
        }
    }

    println!("[send] {} fichiers envoyes, attente de la confirmation finale", sent_files);
    Ok(())
}

fn usage(exe: &str) {
    eprintln!("Utilisation :");
    eprintln!("  {} receive <adresse:port> <repertoire_sortie> [--upnp] [--open-fw] [--close-fw]", exe);
    eprintln!("  {} send <repertoire> <adresse:port>", exe);
    eprintln!();
    eprintln!("Options du recepteur :");
    eprintln!("  --upnp        ouvre automatiquement la redirection de port sur la box (UPnP-IGD)");
    eprintln!("  --open-fw     ouvre la regle entrante dans le pare-feu Windows (necessite administrateur)");
    eprintln!("  --close-fw    ferme pare-feu + redirection UPnP apres le transfert");
    eprintln!();
    eprintln!("Exemples :");
    eprintln!("  {} receive 0.0.0.0:{} D:\\reception --upnp --open-fw --close-fw", exe, DEFAULT_PORT);
    eprintln!("  {} send D:\\donnees 82.65.12.34:{}", exe, DEFAULT_PORT);
    eprintln!();
    eprintln!("Ordre : demarrez d'abord le recepteur, puis l'envoyeur.");
    eprintln!("Reprise : relancez les deux cotes, les fichiers deja transferes (checksum valide) sont ignores.");
}

#[derive(Default)]
struct Opts {
    upnp: bool,
    open_fw: bool,
    close_fw_after: bool,
}

fn parse_opts(args: &[String]) -> (Vec<String>, Opts) {
    let mut pos = Vec::new();
    let mut o = Opts::default();
    for a in args {
        match a.as_str() {
            "--upnp" => o.upnp = true,
            "--open-fw" | "--open-fw-after" => o.open_fw = true,
            "--close-fw" | "--close-fw-after" => o.close_fw_after = true,
            _ => pos.push(a.clone()),
        }
    }
    (pos, o)
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let exe = argv.first().map(|s| s.as_str()).unwrap_or("dirtransfer");
    let (args, opts) = parse_opts(&argv[1..]);
    let res = match args.first().map(|s| s.as_str()) {
        Some("receive") => match (args.get(1), args.get(2)) {
            (Some(addr), Some(out)) => cmd_receive(addr, Path::new(out), &opts),
            _ => {
                usage(exe);
                std::process::exit(2);
            }
        },
        Some("send") => match (args.get(1), args.get(2)) {
            (Some(dir), Some(addr)) => cmd_send(Path::new(dir), addr),
            _ => {
                usage(exe);
                std::process::exit(2);
            }
        },
        _ => {
            usage(exe);
            std::process::exit(2);
        }
    };
    if let Err(e) = res {
        eprintln!("[erreur] {}", e);
        std::process::exit(1);
    }
}

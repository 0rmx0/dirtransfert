use crate::config;
use crate::transfer::{self, Opts, Reporter};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const DEFAULT_PORT: u16 = 47500;

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

struct ConsoleReporter {
    json: bool,
    last_progress: Mutex<Option<Instant>>,
}

impl Reporter for ConsoleReporter {
    fn log(&self, msg: String) {
        if self.json {
            println!("{{\"type\":\"log\",\"msg\":\"{}\"}}", json_escape(&msg));
        } else {
            println!("{}", msg);
        }
    }
    fn scan(&self, done: usize, total: usize, current: &str) {
        if self.json {
            return;
        }
        print!("\r[analyse] {}/{} fichiers — {}   ", done, total, current);
        let _ = std::io::stdout().flush();
    }
    fn progress(&self, sent: u64, total: u64, speed_mbps: f64, pct: u64) {
        if self.json {
            let mut last = self.last_progress.lock().unwrap();
            let now = Instant::now();
            let due = last
                .map(|t| now.duration_since(t) >= Duration::from_secs(1))
                .unwrap_or(true);
            if due {
                *last = Some(now);
                drop(last);
                println!(
                    "{{\"type\":\"progress\",\"sent\":{},\"total\":{},\"speed_mbps\":{:.3},\"pct\":{}}}",
                    sent, total, speed_mbps, pct
                );
            }
            return;
        }
        print!(
            "\r[{}] / {} ({:.0} Mo/s) {}%   ",
            transfer::human(sent),
            transfer::human(total),
            speed_mbps,
            pct
        );
        let _ = std::io::stdout().flush();
    }
    fn done(&self, ok: bool, files_ok: usize, files_total: usize) {
        if self.json {
            println!(
                "{{\"type\":\"done\",\"ok\":{},\"files_ok\":{},\"files_total\":{}}}",
                ok, files_ok, files_total
            );
        } else {
            println!();
            if ok {
                println!("[ok] {}/{} fichiers valides", files_ok, files_total);
            }
        }
    }
    fn confirm_manifest(&self, root_name: &str, files: &[(String, u64)], total: u64) -> bool {
        println!();
        println!(
            "L'emetteur propose de transferer \"{}\" : {} fichiers, {}",
            root_name,
            files.len(),
            transfer::human(total)
        );
        let max_show = 20.min(files.len());
        for (rel, size) in files.iter().take(max_show) {
            println!("  {} ({})", rel, transfer::human(*size));
        }
        if files.len() > max_show {
            println!("  ... et {} autres fichiers", files.len() - max_show);
        }
        eprint!("Accepter ce transfert ? [o/N] ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(_) => {
                let a = line.trim().to_ascii_lowercase();
                a == "o" || a == "oui" || a == "y" || a == "yes"
            }
            Err(_) => false,
        }
    }
}

/// QR code en ASCII (terminal) du payload fourni.
pub fn qr_ascii(payload: &str) -> Result<String, String> {
    let code = qrcode::QrCode::new(payload).map_err(|e| format!("qrcode : {}", e))?;
    let w = code.width();
    let mut s = String::with_capacity(w * w * 4 + w);
    for y in 0..w {
        for x in 0..w {
            s.push_str(if matches!(code[(x, y)], qrcode::types::Color::Dark) {
                "██"
            } else {
                "  "
            });
        }
        s.push('\n');
    }
    Ok(s)
}

fn usage(exe: &str) {
    eprintln!("Utilisation :");
    eprintln!("  {} --cli receive <adresse:port> <repertoire_sortie> [--code XXXXX-XXXXX] [--yes] [--upnp] [--open-fw] [--close-fw]", exe);
    eprintln!("  {} --cli send <repertoire> <adresse:port> [adresse:port ...] --code XXXXX-XXXXX [--discover] [--dry-run]", exe);
    eprintln!(
        "  {} --cli verify <repertoire_source> <repertoire_reception>",
        exe
    );
    eprintln!();
    eprintln!("Sans --cli : interface graphique.");
    eprintln!();
    eprintln!("Options communes :");
    eprintln!("  --code XXXXX-XXXXX  code de jumelage 10 caracteres (obligatoire pour send, genere pour receive si omis)");
    eprintln!("  --json              sortie machine (lignes JSON sur stdout)");
    eprintln!(
        "  --exclude <motif>   exclut les fichiers correspondant au motif (* et ?), repetable"
    );
    eprintln!("  --limit <Mo/s>      limite de debit de l'envoi");
    eprintln!(
        "  --compress          compresse chaque morceau (zstd) — necessite la feature `compress`"
    );
    eprintln!();
    eprintln!("Options du recepteur :");
    eprintln!("  --yes         accepte automatiquement le manifeste recu (sinon confirmation interactive)");
    eprintln!(
        "  --upnp        redirection de port automatique sur la box (UPnP-IGD, repli NAT-PMP)"
    );
    eprintln!("  --open-fw     regle entrante automatique dans le pare-feu Windows (admin requis)");
    eprintln!("  --close-fw    ferme pare-feu + redirection apres le transfert");
    eprintln!("  --pipeline N  requetes en vol simultanees (1-64, defaut 8)");
    eprintln!("  --ipv6        ecoute sur l'adresse IPv6 globale si disponible");
    eprintln!();
    eprintln!("Options de l'emetteur :");
    eprintln!("  --discover    trouve les recepteurs du reseau local (sonde UDP, code requis) au lieu de saisir l'adresse");
    eprintln!("  --dry-run     liste les fichiers et la taille totale sans rien envoyer");
    eprintln!();
    eprintln!("Configuration : ~/.config/dirtransfert/config (cles : pipeline, exclude, limit, auto-yes).");
    eprintln!();
    eprintln!("Exemples :");
    eprintln!(
        "  {} --cli receive 0.0.0.0:{} /reception --upnp --open-fw --close-fw",
        exe, DEFAULT_PORT
    );
    eprintln!(
        "  {} --cli send /donnees 82.65.12.34:{} --code AB2CD-34567",
        exe, DEFAULT_PORT
    );
    eprintln!(
        "  {} --cli send /donnees --discover --code AB2CD-34567",
        exe
    );
}

pub fn run(argv: &[String]) -> i32 {
    let exe = argv.first().map(|s| s.as_str()).unwrap_or("DirTransfert");
    let cfg = config::load();
    let mut opts = Opts::default();
    let mut json = false;
    let mut discover_mode = false;
    let mut args: Vec<String> = Vec::new();
    let mut code: Option<String> = None;
    let mut cli_exclude: Vec<String> = Vec::new();
    let mut cli_limit: Option<f64> = None;
    let mut it = argv.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--cli" | "--console" => {}
            "--json" => json = true,
            "--yes" => opts.auto_yes = true,
            "--discover" => discover_mode = true,
            "--dry-run" => opts.dry_run = true,
            "--compress" => opts.compress = true,
            "--upnp" => opts.upnp = true,
            "--ipv6" => opts.prefer_ipv6 = true,
            "--open-fw" | "--open-fw-after" => opts.open_fw = true,
            "--close-fw" | "--close-fw-after" => opts.close_fw_after = true,
            "--pipeline" => {
                match it.next().and_then(|v| v.parse::<usize>().ok()) {
                    Some(n) => opts.pipeline = Some(n.clamp(1, 64)),
                    None => {
                        eprintln!("[erreur] --pipeline attend un nombre entre 1 et 64 (ex. --pipeline 16)");
                        return 2;
                    }
                }
            }
            "--exclude" => match it.next() {
                Some(v) if !v.trim().is_empty() => cli_exclude.push(v.trim().to_string()),
                _ => {
                    eprintln!("[erreur] --exclude attend un motif (ex. --exclude '*.tmp')");
                    return 2;
                }
            },
            "--limit" => match it.next().and_then(|v| v.parse::<f64>().ok()) {
                Some(x) if x > 0.0 => cli_limit = Some(x),
                _ => {
                    eprintln!("[erreur] --limit attend un debit en Mo/s > 0 (ex. --limit 50)");
                    return 2;
                }
            },
            "--code" => match it.next().and_then(|v| crate::crypto::norm_code(v)) {
                Some(c) => code = Some(c),
                None => {
                    eprintln!("[erreur] --code attend un code de 10 caracteres (ex. AB2CD-34567)");
                    return 2;
                }
            },
            _ => args.push(a.clone()),
        }
    }
    if opts.pipeline.is_none() {
        opts.pipeline = cfg.pipeline;
    }
    if cli_exclude.is_empty() {
        opts.exclude = cfg.exclude.clone();
    } else {
        opts.exclude = cli_exclude;
    }
    if cli_limit.is_none() {
        opts.limit_mbps = cfg.limit_mbps;
    } else {
        opts.limit_mbps = cli_limit;
    }
    if cfg.auto_yes {
        opts.auto_yes = true;
    }

    let rep = ConsoleReporter {
        json,
        last_progress: Mutex::new(None),
    };
    let res: Result<(), String> = match args.first().map(|s| s.as_str()) {
        Some("receive") => match (args.get(1), args.get(2)) {
            (Some(addr), Some(out)) => {
                let code = match code {
                    Some(c) => c,
                    None => {
                        let c = crate::crypto::gen_code();
                        println!("code de jumelage : {}  (a saisir sur l'emetteur)", c);
                        c
                    }
                };
                if let Some((_, port)) = transfer::parse_endpoint(addr) {
                    let ip = crate::upnp::local_ip();
                    let host = if ip.is_empty() {
                        "<ip_locale>"
                    } else {
                        ip.as_str()
                    };
                    let payload = format!("dtx://{}:{}?code={}", host, port, code);
                    match qr_ascii(&payload) {
                        Ok(qr) => {
                            println!("QR code (dtx://{}:{}?code=...) :", host, port);
                            println!("{}", qr);
                        }
                        Err(e) => eprintln!("[info] QR indisponible : {}", e),
                    }
                }
                let r = transfer::receive(addr, Path::new(out), &code, &opts, &rep);
                config::append_history(&format!("receive {} out={} ok={}", addr, out, r.is_ok()));
                r
            }
            _ => {
                usage(exe);
                return 2;
            }
        },
        Some("send") => match args.get(1) {
            Some(dir) => {
                let Some(code) = code else {
                    eprintln!(
                        "[erreur] --code est obligatoire pour envoyer (ex. --code AB2CD-34567)"
                    );
                    return 2;
                };
                let mut addrs: Vec<String> = args.iter().skip(2).cloned().collect();
                if discover_mode || addrs.is_empty() {
                    match crate::discover::discover(&code, Duration::from_secs(3)) {
                        Ok(peers) => {
                            for (ip, port) in &peers {
                                println!("recepteur trouve : {}:{}", ip, port);
                            }
                            addrs = peers
                                .iter()
                                .map(|(ip, port)| format!("{}:{}", ip, port))
                                .collect();
                        }
                        Err(e) => {
                            eprintln!("[erreur] --discover : {}", e);
                            return 1;
                        }
                    }
                }
                let r = transfer::send(Path::new(dir), &addrs, &code, &opts, &rep);
                config::append_history(&format!(
                    "send {} -> {} ok={}",
                    dir,
                    addrs.join(","),
                    r.is_ok()
                ));
                r
            }
            _ => {
                usage(exe);
                return 2;
            }
        },
        Some("verify") => match (args.get(1), args.get(2)) {
            (Some(src), Some(dest)) => transfer::verify(Path::new(src), Path::new(dest), &rep),
            _ => {
                usage(exe);
                return 2;
            }
        },
        _ => {
            usage(exe);
            return 2;
        }
    };
    if let Err(e) = res {
        if json {
            println!("{{\"type\":\"error\",\"msg\":\"{}\"}}", json_escape(&e));
        }
        eprintln!("[erreur] {}", e);
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_escape_basique() {
        assert_eq!(json_escape("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
    }

    #[test]
    fn qr_ascii_genere() {
        let qr = qr_ascii("dtx://192.168.1.10:47500?code=AB2CD-34567").unwrap();
        assert!(qr.contains("██"));
        assert!(qr.lines().count() >= 21);
    }
}

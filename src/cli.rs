use crate::transfer::{self, Opts, Reporter};
use std::io::Write;
use std::path::Path;

const DEFAULT_PORT: u16 = 47500;

struct ConsoleReporter;

impl Reporter for ConsoleReporter {
    fn log(&self, msg: String) {
        println!("{}", msg);
    }
    fn scan(&self, done: usize, total: usize, current: &str) {
        print!("\r[analyse] {}/{} fichiers — {}   ", done, total, current);
        let _ = std::io::stdout().flush();
    }
    fn progress(&self, sent: u64, total: u64, speed_mbps: f64, pct: u64) {
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
        println!();
        if ok {
            println!("[ok] {}/{} fichiers valides", files_ok, files_total);
        }
    }
}

impl ConsoleReporter {
    fn new() -> Self {
        ConsoleReporter
    }
}

fn usage(exe: &str) {
    eprintln!("Utilisation :");
    eprintln!("  {} --cli receive <adresse:port> <repertoire_sortie> [--code ABCD-2345] [--upnp] [--open-fw] [--close-fw]", exe);
    eprintln!(
        "  {} --cli send <repertoire> <adresse:port> --code ABCD-2345",
        exe
    );
    eprintln!();
    eprintln!("Sans --cli : interface graphique.");
    eprintln!();
    eprintln!("Options communes :");
    eprintln!("  --code XXXX-XXXX  code de jumelage (obligatoire, chiffre le flux). Cote recepteur, genere-en un si omis.");
    eprintln!();
    eprintln!("Options du recepteur :");
    eprintln!("  --upnp        ouvre automatiquement la redirection de port sur la box (UPnP-IGD)");
    eprintln!("  --open-fw     ouvre la regle entrante dans le pare-feu Windows (necessite administrateur)");
    eprintln!("  --close-fw    ferme pare-feu + redirection UPnP apres le transfert");
    eprintln!();
    eprintln!("Exemples :");
    eprintln!(
        "  {} --cli receive 0.0.0.0:{} D:\\\\reception --upnp --open-fw --close-fw",
        exe, DEFAULT_PORT
    );
    eprintln!(
        "  {} --cli send D:\\\\donnees 82.65.12.34:{}",
        exe, DEFAULT_PORT
    );
}

pub fn run(argv: &[String]) -> i32 {
    let exe = argv.first().map(|s| s.as_str()).unwrap_or("DirTransfert");
    let mut opts = Opts::default();
    let mut args: Vec<String> = Vec::new();
    let mut code: Option<String> = None;
    let mut it = argv.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--cli" | "--console" => {}
            "--upnp" => opts.upnp = true,
            "--open-fw" | "--open-fw-after" => opts.open_fw = true,
            "--close-fw" | "--close-fw-after" => opts.close_fw_after = true,
            "--code" => match it.next().and_then(|v| crate::crypto::norm_code(v)) {
                Some(c) => code = Some(c),
                None => {
                    eprintln!("[erreur] --code attend un code de 8 caracteres (ex. ABCD-2345)");
                    return 2;
                }
            },
            _ => args.push(a.clone()),
        }
    }
    let rep = ConsoleReporter::new();
    let res = match args.first().map(|s| s.as_str()) {
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
                transfer::receive(addr, Path::new(out), &code, &opts, &rep)
            }
            _ => {
                usage(exe);
                return 2;
            }
        },
        Some("send") => match (args.get(1), args.get(2)) {
            (Some(dir), Some(addr)) => {
                let Some(code) = code else {
                    eprintln!(
                        "[erreur] --code est obligatoire pour envoyer (ex. --code ABCD-2345)"
                    );
                    return 2;
                };
                transfer::send(Path::new(dir), addr, &code, &rep)
            }
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
        eprintln!("[erreur] {}", e);
        return 1;
    }
    0
}

use crate::transfer::{self, Opts, Reporter};
use std::path::Path;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const DEFAULT_PORT: u16 = 47500;

struct ConsoleReporter {
    running: Arc<AtomicBool>,
}

impl Reporter for ConsoleReporter {
    fn log(&self, msg: String) {
        self.running.store(false, Ordering::Relaxed);
        println!("{}", msg);
    }
    fn progress(&self, sent: u64, total: u64, speed_mbps: f64, pct: u64) {
        print!("\r[{}] / {} ({:.0} Mo/s) {}%   ", transfer::human(sent), transfer::human(total), speed_mbps, pct);
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
    fn new() -> (Self, Arc<AtomicBool>) {
        let running = Arc::new(AtomicBool::new(true));
        let r = ConsoleReporter { running: running.clone() };
        (r, running)
    }
}

impl Drop for ConsoleReporter {
    fn drop(&mut self) {}
}

fn usage(exe: &str) {
    eprintln!("Utilisation :");
    eprintln!("  {} --cli receive <adresse:port> <repertoire_sortie> [--upnp] [--open-fw] [--close-fw]", exe);
    eprintln!("  {} --cli send <repertoire> <adresse:port>", exe);
    eprintln!();
    eprintln!("Sans --cli : interface graphique.");
    eprintln!();
    eprintln!("Options du recepteur :");
    eprintln!("  --upnp        ouvre automatiquement la redirection de port sur la box (UPnP-IGD)");
    eprintln!("  --open-fw     ouvre la regle entrante dans le pare-feu Windows (necessite administrateur)");
    eprintln!("  --close-fw    ferme pare-feu + redirection UPnP apres le transfert");
    eprintln!();
    eprintln!("Exemples :");
    eprintln!("  {} --cli receive 0.0.0.0:{} D:\\\\reception --upnp --open-fw --close-fw", exe, DEFAULT_PORT);
    eprintln!("  {} --cli send D:\\\\donnees 82.65.12.34:{}", exe, DEFAULT_PORT);
}

pub fn run(argv: &[String]) -> i32 {
    let exe = argv.first().map(|s| s.as_str()).unwrap_or("dirtransfer");
    let mut opts = Opts::default();
    let mut args: Vec<String> = Vec::new();
    for a in argv.iter().skip(1) {
        match a.as_str() {
            "--cli" | "--console" => {}
            "--upnp" => opts.upnp = true,
            "--open-fw" | "--open-fw-after" => opts.open_fw = true,
            "--close-fw" | "--close-fw-after" => opts.close_fw_after = true,
            _ => args.push(a.clone()),
        }
    }
    let (rep, _tick_ctrl) = ConsoleReporter::new();
    let res = match args.first().map(|s| s.as_str()) {
        Some("receive") => match (args.get(1), args.get(2)) {
            (Some(addr), Some(out)) => transfer::receive(addr, Path::new(out), &opts, &rep),
            _ => {
                usage(exe);
                return 2;
            }
        },
        Some("send") => match (args.get(1), args.get(2)) {
            (Some(dir), Some(addr)) => transfer::send(Path::new(dir), addr, &rep),
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

//! Interface graphique Slint de DirTransfert (logique ; l'UI est dans `ui.rs`)

use crate::crypto;
use crate::discover;
use crate::transfer::{self, Opts, Reporter};
use crate::ui::AppWindow;
use rfd::FileDialog;
use slint::{ComponentHandle, Image, SharedPixelBuffer, Weak};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const LOG_MAX_LINES: usize = 400;

#[derive(Debug)]
enum UiEvent {
    Log(String),
    BrowseDir(String),
    BrowseOutDir(String),
    SetRunning(bool),
    SetCode(String),
    Progress {
        sent: u64,
        total: u64,
        speed_mbps: f64,
        pct: u64,
    },
    AwaitingApproval(String),
    ApprovalResolved,
    Discovered(Vec<String>),
    SetQr(Vec<slint::Rgba8Pixel>, u32),
}

fn send_event(ui: &Weak<AppWindow>, ev: UiEvent) {
    let w = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = w.upgrade() {
            process_event(&ui, ev);
        }
    });
}

/// Point d'approbation du manifeste côté UI (bloquant pour le thread récepteur).
struct Approval {
    state: Mutex<Option<bool>>,
    cv: Condvar,
}

impl Approval {
    fn new() -> Arc<Self> {
        Arc::new(Approval {
            state: Mutex::new(None),
            cv: Condvar::new(),
        })
    }
    fn decide(&self, ok: bool) {
        *self.state.lock().unwrap() = Some(ok);
        self.cv.notify_all();
    }
    fn wait(&self) -> bool {
        let mut g = self.state.lock().unwrap();
        loop {
            if let Some(v) = *g {
                return v;
            }
            g = self.cv.wait(g).unwrap();
        }
    }
}

struct SlintReporter {
    ui: Weak<AppWindow>,
    last_progress: Mutex<Instant>,
    approval: Option<Arc<Approval>>,
}

impl Reporter for SlintReporter {
    fn log(&self, msg: String) {
        send_event(&self.ui, UiEvent::Log(msg));
    }
    fn scan(&self, done: usize, total: usize, current: &str) {
        send_event(
            &self.ui,
            UiEvent::Log(format!("Analyse: {}/{}: {}", done, total, current)),
        );
    }
    fn progress(&self, sent: u64, total: u64, speed_mbps: f64, pct: u64) {
        // Throttle : un événement de progression toutes les 100 ms max.
        let mut last = self.last_progress.lock().unwrap();
        let now = Instant::now();
        if now.duration_since(*last) < Duration::from_millis(100) {
            return;
        }
        *last = now;
        drop(last);
        send_event(
            &self.ui,
            UiEvent::Progress {
                sent,
                total,
                speed_mbps,
                pct,
            },
        );
    }
    fn done(&self, ok: bool, files_ok: usize, files_total: usize) {
        let msg = if ok {
            format!("[ok] {}/{} fichiers valides", files_ok, files_total)
        } else {
            format!(
                "[erreur] Transfert incomplet ({}/{} ok)",
                files_ok, files_total
            )
        };
        send_event(&self.ui, UiEvent::Log(msg));
    }
    fn confirm_manifest(&self, root_name: &str, files: &[(String, u64)], total: u64) -> bool {
        let mut summary = format!(
            "\"{}\" : {} fichiers, {} :\n",
            root_name,
            files.len(),
            transfer::human(total)
        );
        for (rel, size) in files.iter().take(20) {
            summary.push_str(&format!("  {} ({})\n", rel, transfer::human(*size)));
        }
        if files.len() > 20 {
            summary.push_str(&format!("  ... et {} autres fichiers", files.len() - 20));
        }
        send_event(&self.ui, UiEvent::AwaitingApproval(summary));
        let Some(ap) = &self.approval else {
            return true;
        };
        let ok = ap.wait();
        send_event(&self.ui, UiEvent::ApprovalResolved);
        ok
    }
}

fn qr_payload(ui: &AppWindow) -> Option<String> {
    let code: String = ui.get_code().into();
    crypto::norm_code(&code)?;
    let addr: String = ui.get_addr().into();
    let (_, port) = transfer::parse_endpoint(&addr)?;
    let ip = crate::upnp::local_ip();
    let host = if ip.is_empty() {
        "<ip_locale>".to_string()
    } else {
        ip
    };
    Some(format!("dtx://{}:{}?code={}", host, port, code))
}

fn qr_image(payload: &str) -> Option<(Vec<slint::Rgba8Pixel>, u32)> {
    let code = qrcode::QrCode::new(payload).ok()?;
    let w = code.width();
    let scale: usize = 4;
    let dim = w * scale;
    let light = slint::Rgba8Pixel {
        r: 17,
        g: 17,
        b: 17,
        a: 255,
    };
    let dark = slint::Rgba8Pixel {
        r: 0,
        g: 255,
        b: 127,
        a: 255,
    };
    let mut pixels = vec![light; dim * dim];
    for y in 0..w {
        for x in 0..w {
            if matches!(code[(x, y)], qrcode::types::Color::Dark) {
                for dy in 0..scale {
                    for dx in 0..scale {
                        pixels[(y * scale + dy) * dim + (x * scale + dx)] = dark;
                    }
                }
            }
        }
    }
    Some((pixels, dim as u32))
}

fn refresh_qr(ui: &Weak<AppWindow>) {
    let Some(ui) = ui.upgrade() else { return };
    if let Some(payload) = qr_payload(&ui) {
        if let Some((pixels, dim)) = qr_image(&payload) {
            send_event(&ui.as_weak(), UiEvent::SetQr(pixels, dim));
        }
    }
}

pub fn run() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    ui.set_code(crypto::gen_code().into());
    ui.set_pipeline_text("N=8".into());

    let cancel_flag = Arc::new(AtomicBool::new(false));
    let approval = Approval::new();

    let w = ui.as_weak();
    ui.on_browse_dir(move || {
        let w = w.clone();
        thread::spawn(move || {
            if let Some(path) = FileDialog::new().pick_folder() {
                send_event(&w, UiEvent::BrowseDir(path.display().to_string()));
            }
        });
    });

    let w = ui.as_weak();
    ui.on_browse_out_dir(move || {
        let w = w.clone();
        thread::spawn(move || {
            if let Some(path) = FileDialog::new().pick_folder() {
                send_event(&w, UiEvent::BrowseOutDir(path.display().to_string()));
            }
        });
    });

    let w = ui.as_weak();
    ui.on_generate_code(move || {
        let c = crypto::gen_code();
        send_event(&w, UiEvent::SetCode(c));
    });

    let w = ui.as_weak();
    ui.on_code_edited(move |text| {
        if let Some(ui) = w.upgrade() {
            ui.set_code(text);
            refresh_qr(&ui.as_weak());
        }
    });

    const DEFAULT_LISTEN_ADDR: &str = "0.0.0.0:47500";
    let w = ui.as_weak();
    ui.on_mode_set(move |mode| {
        if let Some(ui) = w.upgrade() {
            let prev: String = ui.get_mode().into();
            let mode_str: String = mode.to_string();
            if prev != mode_str {
                if mode == "receive" && ui.get_addr().is_empty() {
                    ui.set_addr(DEFAULT_LISTEN_ADDR.into());
                }
                if mode == "send" {
                    let addr: String = ui.get_addr().into();
                    if addr == DEFAULT_LISTEN_ADDR {
                        ui.set_addr("".into());
                    }
                }
                ui.set_mode(mode.clone());
                if mode == "receive" {
                    refresh_qr(&ui.as_weak());
                }
            }
        }
    });

    let w = ui.as_weak();
    ui.on_dir_edited(move |text| {
        if let Some(ui) = w.upgrade() {
            if ui.get_mode() == "send" {
                ui.set_dir(text);
            } else {
                ui.set_out_dir(text);
            }
        }
    });

    let w = ui.as_weak();
    let cancel_handle = cancel_flag.clone();
    let approval_start = approval.clone();
    ui.on_start_transfer(move || {
        if let Some(ui) = w.upgrade() {
            cancel_handle.store(false, Ordering::SeqCst);
            start_transfer(&ui, cancel_handle.clone(), approval_start.clone());
        }
    });

    let w = ui.as_weak();
    let cancel_handle = cancel_flag.clone();
    ui.on_cancel_transfer(move || {
        cancel_handle.store(true, Ordering::SeqCst);
        send_event(&w, UiEvent::Log("Annulation demandée...".into()));
    });

    let w = ui.as_weak();
    let approval_handle = approval.clone();
    ui.on_accept_transfer(move || {
        approval_handle.decide(true);
        if let Some(ui) = w.upgrade() {
            ui.set_awaiting_approval(false);
        }
    });

    let w = ui.as_weak();
    let approval_handle = approval.clone();
    ui.on_refuse_transfer(move || {
        approval_handle.decide(false);
        if let Some(ui) = w.upgrade() {
            ui.set_awaiting_approval(false);
        }
    });

    let w = ui.as_weak();
    ui.on_copy_command(move || {
        if let Some(ui) = w.upgrade() {
            let code: String = ui.get_code().into();
            let addr: String = ui.get_addr().into();
            let cmd = match transfer::parse_endpoint(&addr) {
                Some((_, port)) => {
                    let ip = crate::upnp::local_ip();
                    let host = if ip.is_empty() {
                        "<ip_locale>".to_string()
                    } else {
                        ip
                    };
                    format!(
                        "DirTransfert --cli send <repertoire> {}:{} --code {}",
                        host, port, code
                    )
                }
                None => String::new(),
            };
            ui.set_cmd_text(cmd.into());
            refresh_qr(&ui.as_weak());
        }
    });

    let w = ui.as_weak();
    ui.on_discover(move || {
        let w = w.clone();
        let code = match w.upgrade() {
            Some(ui) => ui.get_code().to_string(),
            None => return,
        };
        if crypto::norm_code(&code).is_none() {
            send_event(
                &w,
                UiEvent::Log(
                    "[erreur] Code invalide : 10 caractères sans O/0/1/I (ex. AB2CD-34567)".into(),
                ),
            );
            return;
        }
        send_event(
            &w,
            UiEvent::Log("Découverte des récepteurs sur le réseau local...".into()),
        );
        thread::spawn(
            move || match discover::discover(&code, Duration::from_secs(3)) {
                Ok(peers) => send_event(
                    &w,
                    UiEvent::Discovered(
                        peers
                            .iter()
                            .map(|(ip, port)| format!("{}:{}", ip, port))
                            .collect(),
                    ),
                ),
                Err(e) => send_event(&w, UiEvent::Log(format!("[erreur] découverte : {}", e))),
            },
        );
    });

    refresh_qr(&ui.as_weak());
    ui.run()?;
    Ok(())
}

fn process_event(ui: &AppWindow, event: UiEvent) {
    match event {
        UiEvent::Log(msg) => {
            let mut log: String = ui.get_log_text().into();
            log.push_str(&msg);
            log.push('\n');
            let count = log.lines().count();
            if count > LOG_MAX_LINES {
                let tail: Vec<&str> = log.lines().skip(count - LOG_MAX_LINES).collect();
                log = tail.join("\n");
                log.push('\n');
            }
            ui.set_log_text(log.into());
        }
        UiEvent::BrowseDir(path) => {
            if ui.get_mode() == "send" {
                ui.set_dir(path.into());
            } else {
                ui.set_out_dir(path.into());
            }
        }
        UiEvent::BrowseOutDir(path) => {
            ui.set_out_dir(path.into());
        }
        UiEvent::SetRunning(running) => {
            ui.set_running(running);
        }
        UiEvent::SetCode(code) => {
            ui.set_code(code.into());
            refresh_qr(&ui.as_weak());
        }
        UiEvent::Progress {
            sent,
            total,
            speed_mbps,
            pct,
        } => {
            ui.set_progress((pct as f32 / 100.0).clamp(0.0, 1.0));
            ui.set_progress_text(
                format!(
                    "{} / {} — {:.1} Mo/s — {}%",
                    transfer::human(sent),
                    transfer::human(total),
                    speed_mbps,
                    pct
                )
                .into(),
            );
        }
        UiEvent::AwaitingApproval(summary) => {
            ui.set_manifest_text(summary.into());
            ui.set_awaiting_approval(true);
        }
        UiEvent::ApprovalResolved => {
            ui.set_awaiting_approval(false);
        }
        UiEvent::Discovered(addrs) => {
            if addrs.is_empty() {
                ui.set_log_text(
                    format!("{}{}\n", ui.get_log_text(), "[info] aucun récepteur trouvé").into(),
                );
            } else {
                for a in &addrs {
                    let log: String = ui.get_log_text().into();
                    ui.set_log_text(format!("{}récepteur trouvé : {}\n", log, a).into());
                }
                ui.set_addr(addrs[0].clone().into());
            }
        }
        UiEvent::SetQr(pixels, dim) => {
            let mut buf = SharedPixelBuffer::<slint::Rgba8Pixel>::new(dim, dim);
            buf.make_mut_slice().copy_from_slice(&pixels);
            ui.set_qr_image(Image::from_rgba8(buf));
            ui.set_qr_visible(true);
        }
    }
}

fn start_transfer(ui: &AppWindow, cancel: Arc<AtomicBool>, approval: Arc<Approval>) {
    let mode_send = ui.get_mode() == "send";
    let code: String = ui.get_code().into();
    let addr: String = ui.get_addr().into();
    let upnp = ui.get_upnp();
    let open_fw = ui.get_open_fw();
    let close_fw = ui.get_close_fw();
    let prefer_ipv6 = ui.get_prefer_ipv6();
    let pipeline = ui.get_pipeline() as usize;
    let dir: String = if mode_send {
        ui.get_dir().into()
    } else {
        ui.get_out_dir().into()
    };
    let exclude: Vec<String> = ui
        .get_exclude_text()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let limit_mbps = ui
        .get_limit_text()
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|x| *x > 0.0);

    let w = ui.as_weak();
    if crypto::norm_code(&code).is_none() {
        send_event(
            &w,
            UiEvent::Log(
                "[erreur] Code invalide : 10 caractères sans O/0/1/I (ex. AB2CD-34567)".into(),
            ),
        );
        return;
    }
    if dir.is_empty() {
        send_event(
            &w,
            UiEvent::Log(if mode_send {
                "[erreur] Répertoire à envoyer non spécifié".into()
            } else {
                "[erreur] Dossier de réception non spécifié".into()
            }),
        );
        return;
    }
    if addr.is_empty() {
        send_event(&w, UiEvent::Log("[erreur] Adresse non spécifiée".into()));
        return;
    }
    if transfer::parse_endpoint(&addr).is_none() {
        send_event(
            &w,
            UiEvent::Log(format!("[erreur] Adresse invalide : {}", addr)),
        );
        return;
    }

    send_event(&w, UiEvent::SetRunning(true));
    send_event(&w, UiEvent::Log("Démarrage du transfert...".into()));
    send_event(&w, UiEvent::ApprovalResolved);

    let opts = Opts {
        upnp,
        open_fw,
        close_fw_after: close_fw,
        pipeline: Some(pipeline),
        prefer_ipv6,
        cancel: Some(cancel.clone()),
        auto_yes: false,
        exclude,
        dry_run: false,
        limit_mbps,
        compress: false,
    };

    thread::spawn(move || {
        let rep = SlintReporter {
            ui: w.clone(),
            last_progress: Mutex::new(Instant::now()),
            approval: if mode_send { None } else { Some(approval) },
        };
        let path = PathBuf::from(dir);
        let result = if mode_send {
            transfer::send(&path, &[addr], &code, &opts, &rep)
        } else {
            transfer::receive(&addr, &path, &code, &opts, &rep)
        };
        if let Err(e) = result {
            send_event(&w, UiEvent::Log(format!("[erreur] {}", e)));
        }
        send_event(&w, UiEvent::SetRunning(false));
        send_event(&w, UiEvent::ApprovalResolved);
    });
}

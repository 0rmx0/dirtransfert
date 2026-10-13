use crate::transfer::{self, Opts, Reporter};
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

enum Event {
    Log(String),
    Progress { sent: u64, total: u64, speed_mbps: f64, pct: u64 },
    Done { ok: bool, files_ok: usize, files_total: usize },
}

struct GuiReporter {
    tx: Sender<Event>,
}

impl Reporter for GuiReporter {
    fn log(&self, msg: String) {
        let _ = self.tx.send(Event::Log(msg));
    }
    fn progress(&self, sent: u64, total: u64, speed_mbps: f64, pct: u64) {
        let _ = self.tx.send(Event::Progress { sent, total, speed_mbps, pct });
    }
    fn done(&self, ok: bool, files_ok: usize, files_total: usize) {
        let _ = self.tx.send(Event::Done { ok, files_ok, files_total });
    }
}

#[derive(Clone, Copy, PartialEq)]
#[derive(Debug)]
enum Mode {
    Send,
    Receive,
}

pub struct App {
    mode: Mode,
    dir: String,
    addr: String,
    out_dir: String,
    upnp: bool,
    open_fw: bool,
    close_fw: bool,
    running: bool,
    error: Option<String>,
    finished: Option<(bool, usize, usize)>,
    sent: u64,
    total: u64,
    speed_mbps: f64,
    pct: u64,
    log: String,
    rx: Option<Receiver<Event>>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            mode: Mode::Receive,
            dir: String::new(),
            addr: format!("0.0.0.0:{}", crate::protocol::DEFAULT_PORT),
            out_dir: String::new(),
            upnp: true,
            open_fw: true,
            close_fw: true,
            running: false,
            error: None,
            finished: None,
            sent: 0,
            total: 0,
            speed_mbps: 0.0,
            pct: 0,
            log: String::new(),
            rx: None,
        }
    }
}

impl App {
    fn start_transfer(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        self.running = true;
        self.error = None;
        self.finished = None;
        self.sent = 0;
        self.total = 0;
        self.speed_mbps = 0.0;
        self.pct = 0;
        self.log.clear();

        let rep = Arc::new(GuiReporter { tx: tx.clone() });
        let mode = self.mode;
        let dir = self.dir.clone();
        let addr = self.addr.clone();
        let out_dir = self.out_dir.clone();
        let opts = Opts {
            upnp: self.upnp,
            open_fw: self.open_fw,
            close_fw_after: self.close_fw,
        };
        let tx_err = tx.clone();
        std::thread::spawn(move || {
            let res = match mode {
                Mode::Send => {
                    let p = PathBuf::from(&dir);
                    transfer::send(&p, &addr, rep.as_ref())
                }
                Mode::Receive => {
                    let p = PathBuf::from(&out_dir);
                    transfer::receive(&addr, &p, &opts, rep.as_ref())
                }
            };
            if let Err(e) = res {
                let _ = tx_err.send(Event::Log(format!("[erreur] {}", e)));
                let _ = tx_err.send(Event::Done { ok: false, files_ok: 0, files_total: 0 });
            }
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(Event::Log(m)) => {
                    self.log.push_str(&m);
                    self.log.push('\n');
                }
                Ok(Event::Progress { sent, total, speed_mbps, pct }) => {
                    self.sent = sent;
                    self.total = total;
                    self.speed_mbps = speed_mbps;
                    self.pct = pct;
                }
                Ok(Event::Done { ok, files_ok, files_total }) => {
                    self.running = false;
                    self.finished = Some((ok, files_ok, files_total));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.running = false;
                    self.rx = None;
                    break;
                }
            }
        }
        if self.running {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("dirtransfer");
            ui.separator();

            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.mode, Mode::Send, "Envoyer");
                ui.selectable_value(&mut self.mode, Mode::Receive, "Recevoir");
            });
            ui.separator();

            let enabled = !self.running;
            if self.mode == Mode::Send {
                ui.horizontal(|ui| {
                    ui.label("Repertoire a envoyer :");
                    ui.add_enabled(enabled, egui::TextEdit::singleline(&mut self.dir).desired_width(320.0));
                    if ui.add_enabled(enabled, egui::Button::new("Parcourir...")).clicked() {
                        if let Some(p) = rfd::FileDialog::new().pick_folder() {
                            self.dir = p.display().to_string();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Destination (hote:port) :");
                    ui.add_enabled(enabled, egui::TextEdit::singleline(&mut self.addr).desired_width(320.0));
                });
            } else {
                ui.horizontal(|ui| {
                    ui.label("Dossier de reception :");
                    ui.add_enabled(enabled, egui::TextEdit::singleline(&mut self.out_dir).desired_width(320.0));
                    if ui.add_enabled(enabled, egui::Button::new("Parcourir...")).clicked() {
                        if let Some(p) = rfd::FileDialog::new().pick_folder() {
                            self.out_dir = p.display().to_string();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Ecoute sur :");
                    ui.add_enabled(enabled, egui::TextEdit::singleline(&mut self.addr).desired_width(320.0));
                });
                ui.separator();
                ui.add_enabled(enabled, egui::Checkbox::new(&mut self.upnp, "Redirection de port UPnP automatique (box)"));
                ui.add_enabled(enabled, egui::Checkbox::new(&mut self.open_fw, "Ouvrir la regle pare-feu Windows (necessite administrateur)"));
                ui.add_enabled(enabled, egui::Checkbox::new(&mut self.close_fw, "Fermer pare-feu + UPnP apres le transfert"));
            }
            ui.separator();

            let can_start = enabled && match self.mode {
                Mode::Send => !self.dir.trim().is_empty() && transfer::parse_endpoint(&self.addr).is_some(),
                Mode::Receive => !self.out_dir.trim().is_empty() && transfer::parse_endpoint(&self.addr).is_some(),
            };
            if ui.add_enabled(can_start, egui::Button::new(if self.mode == Mode::Send { "Demarrer l'envoi" } else { "Demarrer la reception" })).clicked() {
                self.start_transfer();
            }

            if let Some((ok, n, tot)) = self.finished {
                if ok {
                    ui.colored_label(egui::Color32::from_rgb(32, 160, 32), format!("Transfert termine : {}/{} fichiers valides, checksums verifies", n, tot));
                } else {
                    ui.colored_label(egui::Color32::from_rgb(200, 40, 40), "Transfert incomplet — voir le journal ci-dessous, relancez pour reprendre");
                }
            }
            if let Some(e) = &self.error {
                ui.colored_label(egui::Color32::from_rgb(200, 40, 40), e);
            }

            if self.total > 0 || self.running {
                let frac = if self.total > 0 { self.pct as f32 / 100.0 } else { 0.0 };
                ui.add(egui::ProgressBar::new(frac).show_percentage().text(format!(
                    "{} / {} — {:.1} Mo/s",
                    transfer::human(self.sent),
                    transfer::human(self.total),
                    self.speed_mbps
                )));
            }

            ui.separator();
            ui.label("Journal :");
            egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut self.log).desired_width(f32::INFINITY).desired_rows(12).interactive(false));
            });
        });
    }
}

pub fn launch() -> Result<(), String> {
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([640.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native("dirtransfer", native, Box::new(|_cc| Ok(Box::new(App::default()))))
        .map_err(|e| format!("lancement de la fenetre : {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_app_state_coherent() {
        let a = App::default();
        assert_eq!(a.mode, Mode::Receive);
        assert!(a.upnp && a.open_fw && a.close_fw);
        assert!(!a.running);
        assert!(transfer::parse_endpoint(&a.addr).is_some());
        assert!(a.log.is_empty());
    }

    #[test]
    fn endpoints_valides() {
        assert!(transfer::parse_endpoint("0.0.0.0:47500").is_some());
        assert!(transfer::parse_endpoint("82.65.12.34:47500").is_some());
        assert!(transfer::parse_endpoint("sansport").is_none());
        assert!(transfer::parse_endpoint(":47500").is_none());
        assert!(transfer::parse_endpoint("hote:pasunnombre").is_none());
    }
}

use crate::transfer::{self, Opts, Reporter};
use eframe::egui;
use std::future::Future;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::task::{Context, Poll};

enum Event {
    Log(String),
    Progress { sent: u64, total: u64, speed_mbps: f64, pct: u64 },
    Done { ok: bool, files_ok: usize, files_total: usize },
}

// Charte graphique R_M_X — tokens v1.1
const NEANT: egui::Color32 = egui::Color32::from_rgb(0x11, 0x11, 0x11); // fond
const SURFACE: egui::Color32 = egui::Color32::from_rgb(0x22, 0x22, 0x22); // panneaux
const ENCRE: egui::Color32 = egui::Color32::from_rgb(0xFF, 0xFF, 0xFF); // texte fort
const BRUME: egui::Color32 = egui::Color32::from_rgb(0xCC, 0xCC, 0xCC); // texte secondaire
const SPECTRE: egui::Color32 = egui::Color32::from_rgb(0x00, 0xFF, 0xFF); // cyan : interactif, focus
const ETRANGE: egui::Color32 = egui::Color32::from_rgb(0xFF, 0x00, 0xFF); // magenta : emphase
const TERMINAL: egui::Color32 = egui::Color32::from_rgb(0x00, 0xFF, 0x7F); // vert : machine
const OMBRE: egui::Color32 = egui::Color32::from_rgb(0x55, 0x55, 0x55); // discret

const RMX_ROUNDING: egui::CornerRadius = egui::CornerRadius::same(10);
const RMX_STROKE: f32 = 1.0;

fn rmx_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.panel_fill = NEANT;
    v.window_fill = SURFACE;
    v.extreme_bg_color = egui::Color32::BLACK;
    v.faint_bg_color = egui::Color32::from_rgb(0x18, 0x18, 0x18);
    v.hyperlink_color = SPECTRE;
    v.override_text_color = Some(BRUME);
    v.widgets.noninteractive.bg_fill = SURFACE;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, BRUME);
    v.widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, BRUME);
    v.widgets.inactive.bg_stroke = egui::Stroke::new(RMX_STROKE, SPECTRE);
    v.widgets.inactive.corner_radius = RMX_ROUNDING;
    v.widgets.hovered.bg_fill = egui::Color32::TRANSPARENT;
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, SPECTRE);
    v.widgets.hovered.bg_stroke = egui::Stroke::new(RMX_STROKE, SPECTRE);
    v.widgets.hovered.corner_radius = RMX_ROUNDING;
    v.widgets.active.bg_fill = egui::Color32::TRANSPARENT;
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, SPECTRE);
    v.widgets.active.bg_stroke = egui::Stroke::new(RMX_STROKE, ETRANGE);
    v.widgets.active.corner_radius = RMX_ROUNDING;
    v.widgets.open.bg_fill = SURFACE;
    v.widgets.open.fg_stroke = egui::Stroke::new(1.0, ENCRE);
    v.widgets.open.corner_radius = RMX_ROUNDING;
    v.selection.bg_fill = egui::Color32::from_rgb(0x00, 0x33, 0x33);
    v.selection.stroke = egui::Stroke::new(1.0, SPECTRE);
    v.warn_fg_color = ETRANGE;
    v.error_fg_color = egui::Color32::from_rgb(0xFF, 0x6A, 0x4D);
    v
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
    pick_rx: Option<Receiver<Option<PathBuf>>>,
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
            pick_rx: None,
        }
    }
}

impl App {
    fn browse(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.pick_rx = Some(rx);
        std::thread::spawn(move || {
            let picked = block_on(async {
                rfd::AsyncFileDialog::new().pick_folder().await.map(|h| h.path().to_path_buf())
            });
            let _ = tx.send(picked);
        });
    }

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
        if let Some(rx) = &self.pick_rx {
            match rx.try_recv() {
                Ok(Some(p)) => {
                    match self.mode {
                        Mode::Send => self.dir = p.display().to_string(),
                        Mode::Receive => self.out_dir = p.display().to_string(),
                    }
                    self.pick_rx = None;
                }
                Ok(None) => self.pick_rx = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.pick_rx = None,
            }
        }
        if self.running {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("dirtransfer");
            ui.separator();

            ui.horizontal(|ui| {
                let _ = ui.selectable_value(&mut self.mode, Mode::Send, "Envoyer");
                let _ = ui.selectable_value(&mut self.mode, Mode::Receive, "Recevoir");
            });
            ui.separator();

            let enabled = !self.running;
            if self.mode == Mode::Send {
                ui.horizontal(|ui| {
                    ui.label("Repertoire a envoyer :");
                    ui.add_enabled(enabled, egui::TextEdit::singleline(&mut self.dir).desired_width(320.0));
                    if ui.add_enabled(enabled && self.pick_rx.is_none(), egui::Button::new("Parcourir...")).clicked() {
                        self.browse();
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
                    if ui.add_enabled(enabled && self.pick_rx.is_none(), egui::Button::new("Parcourir...")).clicked() {
                        self.browse();
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
            let start_label = if self.mode == Mode::Send { "Demarrer l'envoi" } else { "Demarrer la reception" };
            if ui.add_enabled(can_start, egui::Button::new(egui::RichText::new(start_label).color(SPECTRE))).clicked() {
                self.start_transfer();
            }

            if let Some((ok, n, tot)) = self.finished {
                if ok {
                    ui.colored_label(TERMINAL, format!("Transfert termine : {}/{} fichiers valides, checksums verifies", n, tot));
                } else {
                    ui.colored_label(ETRANGE, "Transfert incomplet — voir le journal ci-dessous, relancez pour reprendre");
                }
            }
            if let Some(e) = &self.error {
                ui.colored_label(ETRANGE, e);
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
            egui::Frame::new().fill(egui::Color32::BLACK).corner_radius(RMX_ROUNDING).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.log)
                            .font(egui::TextStyle::Monospace)
                            .text_color(TERMINAL)
                            .desired_width(f32::INFINITY)
                            .desired_rows(12)
                            .interactive(false),
                    );
                });
            });

            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                ui.label(egui::RichText::new("cGFzIGRlIGRpZXV4LCBwYXMgZGUgbWFpdHJlcyAtIDBybXgw").monospace().color(OMBRE));
            });
        });
    }
}

fn block_on<F: Future>(fut: F) -> F::Output {
    struct NoopWake;
    impl std::task::Wake for NoopWake {
        fn wake(self: Arc<Self>) {}
    }
    let waker = std::task::Waker::from(Arc::new(NoopWake));
    let mut cx = Context::from_waker(&waker);
    let mut fut = std::pin::pin!(fut);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    }
}

pub fn launch() -> Result<(), String> {
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([640.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native("dirtransfer", native, Box::new(|cc| {
        cc.egui_ctx.set_visuals(rmx_visuals());
        Ok(Box::new(App::default()))
    }))
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

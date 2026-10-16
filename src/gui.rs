use crate::icon::{ICON_H, ICON_RGBA, ICON_W};
use crate::transfer::{self, Opts, Reporter};
use eframe::egui;
use std::future::Future;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::task::{Context, Poll};

enum Event {
    Log(String),
    Scan {
        done: usize,
        total: usize,
        current: String,
    },
    Progress {
        sent: u64,
        total: u64,
        speed_mbps: f64,
        pct: u64,
    },
    Done {
        ok: bool,
        files_ok: usize,
        files_total: usize,
    },
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
    fn scan(&self, done: usize, total: usize, current: &str) {
        let _ = self.tx.send(Event::Scan {
            done,
            total,
            current: current.to_string(),
        });
    }
    fn progress(&self, sent: u64, total: u64, speed_mbps: f64, pct: u64) {
        let _ = self.tx.send(Event::Progress {
            sent,
            total,
            speed_mbps,
            pct,
        });
    }
    fn done(&self, ok: bool, files_ok: usize, files_total: usize) {
        let _ = self.tx.send(Event::Done {
            ok,
            files_ok,
            files_total,
        });
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
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
    code: String,
    code_error: Option<String>,
    running: bool,
    error: Option<String>,
    finished: Option<(bool, usize, usize)>,
    sent: u64,
    total: u64,
    speed_mbps: f64,
    pct: u64,
    scanning: bool,
    scan_done: usize,
    scan_total: usize,
    scan_current: String,
    log: String,
    rx: Option<Receiver<Event>>,
    pick_rx: Option<Receiver<Option<PathBuf>>>,
    cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    pipeline: usize,
    show_about: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            mode: Mode::Receive,
            code: crate::crypto::gen_code(),
            dir: String::new(),
            addr: format!("0.0.0.0:{}", crate::protocol::DEFAULT_PORT),
            out_dir: String::new(),
            upnp: true,
            open_fw: true,
            close_fw: true,
            code_error: None,
            running: false,
            error: None,
            finished: None,
            sent: 0,
            total: 0,
            speed_mbps: 0.0,
            pct: 0,
            scanning: false,
            scan_done: 0,
            scan_total: 0,
            scan_current: String::new(),
            log: String::new(),
            rx: None,
            pick_rx: None,
            cancel: None,
            pipeline: 8,
            show_about: false,
        }
    }
}

impl App {
    fn browse(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.pick_rx = Some(rx);
        std::thread::spawn(move || {
            let picked = block_on(async {
                rfd::AsyncFileDialog::new()
                    .pick_folder()
                    .await
                    .map(|h| h.path().to_path_buf())
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
        self.scanning = false;
        self.scan_done = 0;
        self.scan_total = 0;
        self.scan_current = String::new();
        self.log.clear();

        let rep = Arc::new(GuiReporter { tx: tx.clone() });
        let mode = self.mode;
        let dir = self.dir.clone();
        let addr = self.addr.clone();
        let out_dir = self.out_dir.clone();
        let code = self.code.clone();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        let opts = Opts {
            upnp: self.upnp,
            open_fw: self.open_fw,
            close_fw_after: self.close_fw,
            pipeline: Some(self.pipeline),
            cancel: Some(cancel),
        };
        let tx_err = tx.clone();
        let cancel_send = opts.cancel.clone();
        std::thread::spawn(move || {
            let res = match mode {
                Mode::Send => {
                    let p = PathBuf::from(&dir);
                    transfer::send(&p, &addr, &code, &cancel_send, rep.as_ref())
                }
                Mode::Receive => {
                    let p = PathBuf::from(&out_dir);
                    transfer::receive(&addr, &p, &code, &opts, rep.as_ref())
                }
            };
            if let Err(e) = res {
                let _ = tx_err.send(Event::Log(format!("[erreur] {}", e)));
                let _ = tx_err.send(Event::Done {
                    ok: false,
                    files_ok: 0,
                    files_total: 0,
                });
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
                Ok(Event::Scan {
                    done,
                    total,
                    current,
                }) => {
                    self.scanning = done < total || total == 0;
                    self.scan_done = done;
                    self.scan_total = total;
                    self.scan_current = current;
                }
                Ok(Event::Progress {
                    sent,
                    total,
                    speed_mbps,
                    pct,
                }) => {
                    self.sent = sent;
                    self.total = total;
                    self.speed_mbps = speed_mbps;
                    self.pct = pct;
                }
                Ok(Event::Done {
                    ok,
                    files_ok,
                    files_total,
                }) => {
                    self.running = false;
                    self.scanning = false;
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
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(100));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.pick_rx = None,
            }
        }
        if self.running {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }

        egui::Panel::bottom("rmx_footer")
            .frame(
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .corner_radius(egui::CornerRadius::same(10))
                    .fill(egui::Color32::TRANSPARENT)
            )
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new("IkwnaW5mb3JtYXRpb24sIGMnZXN0IGxlIHBvdXZvaXIuIE1haXMgY29tbWUgdG91dCBwb3V2b2lyLCBpbCB5IGEgY2V1eCBxdWkgdmV1bGVudCBsZSBnYXJkZXIgcG91ciBldXguIiAtIEFhcm9uIFN3YXJ0eg==").monospace().color(OMBRE));
                });
            });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("DirTransfert");
            if ui
                .button(egui::RichText::new("À propos").color(SPECTRE))
                .clicked()
            {
                self.show_about = true;
            }
            ui.separator();

            ui.horizontal(|ui| {
                let _ = ui.selectable_value(&mut self.mode, Mode::Send, "Envoyer");
                let _ = ui.selectable_value(&mut self.mode, Mode::Receive, "Recevoir");
            });
            ui.separator();

            let enabled = !self.running;
            ui.horizontal(|ui| {
                ui.label("Code de jumelage :");
                ui.add_enabled(
                    enabled,
                    egui::TextEdit::singleline(&mut self.code)
                        .desired_width(140.0)
                        .hint_text("XXXX-XXXX"),
                );
                if self.mode == Mode::Receive
                    && ui
                        .add_enabled(enabled, egui::Button::new("Generer"))
                        .clicked()
                {
                    self.code = crate::crypto::gen_code();
                }
                self.code_error = match crate::crypto::norm_code(&self.code) {
                    Some(_) => None,
                    None if self.code.trim().is_empty() => None,
                    None => Some(
                        "code invalide : 8 caracteres sans O/0/1/I (ex. ABCD-2345)".to_string(),
                    ),
                };
            });
            if let Some(e) = &self.code_error {
                ui.colored_label(egui::Color32::from_rgb(0xFF, 0x6A, 0x4D), e);
            }
            ui.separator();
            if self.mode == Mode::Send {
                ui.horizontal(|ui| {
                    ui.label("Repertoire a envoyer :");
                    ui.add_enabled(
                        enabled,
                        egui::TextEdit::singleline(&mut self.dir).desired_width(320.0),
                    );
                    if ui
                        .add_enabled(
                            enabled && self.pick_rx.is_none(),
                            egui::Button::new("Parcourir..."),
                        )
                        .clicked()
                    {
                        self.browse();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Destination (hote:port) :");
                    ui.add_enabled(
                        enabled,
                        egui::TextEdit::singleline(&mut self.addr).desired_width(320.0),
                    );
                });
            } else {
                ui.horizontal(|ui| {
                    ui.label("Dossier de reception :");
                    ui.add_enabled(
                        enabled,
                        egui::TextEdit::singleline(&mut self.out_dir).desired_width(320.0),
                    );
                    if ui
                        .add_enabled(
                            enabled && self.pick_rx.is_none(),
                            egui::Button::new("Parcourir..."),
                        )
                        .clicked()
                    {
                        self.browse();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Ecoute sur :");
                    ui.add_enabled(
                        enabled,
                        egui::TextEdit::singleline(&mut self.addr).desired_width(320.0),
                    );
                });
                ui.separator();
                ui.add_enabled(
                    enabled,
                    egui::Checkbox::new(
                        &mut self.upnp,
                        "Redirection de port UPnP automatique (box)",
                    ),
                );
                ui.add_enabled(
                    enabled,
                    egui::Checkbox::new(
                        &mut self.open_fw,
                        "Ouvrir la regle pare-feu Windows (necessite administrateur)",
                    ),
                );
                ui.add_enabled(
                    enabled,
                    egui::Checkbox::new(
                        &mut self.close_fw,
                        "Fermer pare-feu + UPnP apres le transfert",
                    ),
                );
                ui.horizontal(|ui| {
                    ui.label("Requetes en vol :");
                    ui.add_enabled(
                        enabled,
                        egui::Slider::new(&mut self.pipeline, 1..=64).text("pipeline"),
                    );
                });
            }
            ui.separator();

            let can_start = enabled
                && crate::crypto::norm_code(&self.code).is_some()
                && match self.mode {
                    Mode::Send => {
                        !self.dir.trim().is_empty()
                            && transfer::parse_endpoint(&self.addr).is_some()
                    }
                    Mode::Receive => {
                        !self.out_dir.trim().is_empty()
                            && transfer::parse_endpoint(&self.addr).is_some()
                    }
                };
            let start_label = if self.mode == Mode::Send {
                "Demarrer l'envoi"
            } else {
                "Demarrer la reception"
            };
            if ui
                .add_enabled(
                    can_start,
                    egui::Button::new(egui::RichText::new(start_label).color(SPECTRE)),
                )
                .clicked()
            {
                self.start_transfer();
            }
            if self.running
                && ui
                    .button(egui::RichText::new("Annuler").color(ETRANGE))
                    .clicked()
            {
                if let Some(c) = &self.cancel {
                    c.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                self.log.push_str("annulation demandee...\n");
            }

            if let Some((ok, n, tot)) = self.finished {
                if ok {
                    ui.colored_label(
                        TERMINAL,
                        format!(
                            "Transfert termine : {}/{} fichiers valides, checksums verifies",
                            n, tot
                        ),
                    );
                } else {
                    ui.colored_label(
                        ETRANGE,
                        "Transfert incomplet — voir le journal ci-dessous, relancez pour reprendre",
                    );
                }
            }
            if let Some(e) = &self.error {
                ui.colored_label(ETRANGE, e);
            }

            if self.scanning {
                let frac = if self.scan_total > 0 {
                    self.scan_done as f32 / self.scan_total as f32
                } else {
                    0.0
                };
                ui.add(egui::ProgressBar::new(frac).text(format!(
                    "Analyse : {}/{} fichiers — {}",
                    self.scan_done, self.scan_total, self.scan_current
                )));
            }

            if self.total > 0 || self.running {
                let frac = if self.total > 0 {
                    self.pct as f32 / 100.0
                } else {
                    0.0
                };
                ui.add(egui::ProgressBar::new(frac).show_percentage().text(format!(
                    "{} / {} — {:.1} Mo/s",
                    transfer::human(self.sent),
                    transfer::human(self.total),
                    self.speed_mbps
                )));
            }

            ui.separator();
            ui.label("Journal :");
            egui::Frame::new()
                .fill(egui::Color32::BLACK)
                .corner_radius(RMX_ROUNDING)
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
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
        });

        if self.show_about {
            let mut close = false;
            egui::Window::new("À propos")
                .open(&mut self.show_about)
                .resizable(false)
                .collapsible(false)
                .default_width(460.0)
                .show(ui.ctx(), |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height(ui.ctx().input(|i| i.viewport().monitor_size.map(|s| s.y).unwrap_or(600.0)) * 0.7)
                        .show(ui, |ui| {
                    ui.heading(format!("DirTransfert {}", env!("CARGO_PKG_VERSION")));
                    ui.separator();
                    ui.label(egui::RichText::new("Outil de transfert de répertoire décentralisé — TCP chiffré (ChaCha20-Poly1305), code de jumelage, vérification SHA-256 et reprise.").color(BRUME));
                    ui.add_space(8.0);

                    ui.label(egui::RichText::new("Licence").color(SPECTRE));
                    ui.label("Logiciel libre sous licence MIT.");
                    ui.label(egui::RichText::new("Copyright (c) 2026 R_M_X").color(ENCRE));
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(
                        "Autorisation est accordée, à titre gratuit, à toute personne obtenant une copie de ce logiciel et des fichiers de documentation associés (le \"logiciel\"), de commercialiser le Software sans restriction, y compris sans limitation les droits d'utiliser, copier, modifier, fusionner, publier, distribuer, sous-licencier et/ou vendre des copies du Software, et à permettre aux personnes auxquelles le Software est fourni de le faire, sous réserve des conditions suivantes :"
                    ).monospace().color(BRUME));
                    ui.label(egui::RichText::new(
                        "Le texte de licence ci-dessus et l'avertissement relatif aux droits d'auteur doivent être reproduits dans toutes les copies ou parties substantielles du Software."
                    ).monospace().color(BRUME));
                    ui.add_space(8.0);

                    ui.label(egui::RichText::new("Mentions légales").color(SPECTRE));
                    ui.label(egui::RichText::new("LE LOGICIEL EST FOURNI \"TEL QUEL\", SANS AUCUNE GARANTIE D'AUCUNE SORTE, EXPLICITE OU IMPLICITE, NOTAMMENT LES GARANTIES DE COMMERCIALISATION, D'ADÉQUATION À UN USAGE PARTICULIER ET DE NON-CONTREFAÇON. EN AUCUN CAS LES AUTEURS OU DÉTENTEURS DE DROITS D'AUTEUR NE SERONT RESPONSABLES DE TOUTE RÉCLAMATION, DOMMAGE OU AUTRE RESPONSABILITÉ, QUE CE SOIT DANS UNE ACTION CONTRACTUELLE, DÉLICTUELLE OU AUTRE, PROVENANT DE, HORS OU EN RELATION AVEC LE LOGICIEL OU SON UTILISATION OU AVEC D'AUTRES MOUVEMENTS DU LOGICIEL.").monospace().color(BRUME));
                    ui.add_space(8.0);

                    ui.label(egui::RichText::new("Dépendances").color(SPECTRE));
                    ui.label("eframe/egui, rfd, chacha20poly1305, argon2, rand, sha2 — sous licences MIT et/ou Apache-2.0 (projets RustCrypto, egui et al.).");
                    ui.add_space(8.0);

                    ui.label(egui::RichText::new("Crédits").color(SPECTRE));
                    ui.label("Charte graphique et icône R_M_X. Dépôt : https://github.com/0rmx0/dirtransfert");
                    if ui.button("Fermer").clicked() {
                        close = true;
                    }
                        });
                });
            if close {
                self.show_about = false;
            }
        }
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
    let icon = egui::IconData {
        rgba: ICON_RGBA.to_vec(),
        width: ICON_W,
        height: ICON_H,
    };
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([640.0, 560.0])
        .with_icon(std::sync::Arc::new(icon));

    let run = |renderer: eframe::Renderer| -> Result<(), String> {
        let native = eframe::NativeOptions {
            renderer,
            viewport: viewport.clone(),
            ..Default::default()
        };
        let log_path = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("DirTransfert-erreur.log")))
            .unwrap_or_else(|| std::path::PathBuf::from("DirTransfert-erreur.log"));
        let log_path = std::sync::Arc::new(log_path);
        let lp = log_path.clone();
        eframe::run_native(
            "DirTransfert",
            native,
            Box::new(move |cc| {
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&*lp)
                {
                    let _ = writeln!(
                        f,
                        "fenetre creee, wgpu actif : {}",
                        cc.wgpu_render_state.is_some()
                    );
                }
                cc.egui_ctx.set_visuals(rmx_visuals());
                Ok(Box::new(App::default()))
            }),
        )
        .map_err(|e| format!("lancement de la fenetre (renderer {:?}) : {}", renderer, e))
    };

    let log_step = |msg: String| {
        let path = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("DirTransfert-erreur.log")))
            .unwrap_or_else(|| std::path::PathBuf::from("DirTransfert-erreur.log"));
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(f, "{}", msg);
        }
    };

    log_step(format!(
        "DirTransfert v{} - lancement de la GUI",
        env!("CARGO_PKG_VERSION")
    ));
    log_step("tentative renderer wgpu".to_string());
    let result = run(eframe::Renderer::Wgpu);
    if result.is_ok() {
        log_step("wgpu : fermeture normale".to_string());
        return Ok(());
    }
    log_step("renderer wgpu echoue, tentative glow (OpenGL)".to_string());
    let fallback = run(eframe::Renderer::Glow);
    if fallback.is_err() {
        let msg = format!(
            "wgpu : {}\nglow : {}",
            result.unwrap_err(),
            fallback.unwrap_err()
        );
        log_step(format!("echec de tous les renderers\n{}", msg));
        return Err(msg);
    }
    log_step("glow : fermeture normale".to_string());
    Ok(())
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

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
    File {
        rel: String,
        done: usize,
        total: usize,
    },
    DirInfo {
        files: usize,
        bytes: u64,
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

fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .corner_radius(RMX_ROUNDING)
        .stroke(egui::Stroke::new(
            RMX_STROKE,
            egui::Color32::from_rgb(0x33, 0x33, 0x33),
        ))
        .inner_margin(egui::Margin::same(14))
}

fn field_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(0x18, 0x18, 0x18))
        .corner_radius(RMX_ROUNDING)
        .stroke(egui::Stroke::new(
            RMX_STROKE,
            egui::Color32::from_rgb(0x3A, 0x3A, 0x3A),
        ))
        .inner_margin(egui::Margin::same(10))
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
    fn file(&self, rel: &str, done: usize, total: usize) {
        let _ = self.tx.send(Event::File {
            rel: rel.to_string(),
            done,
            total,
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

#[derive(Clone, Copy, PartialEq, Debug)]
enum Screen {
    Home,
    Prepare,
    Running,
    Finished,
    Interrupted,
}

pub struct App {
    mode: Mode,
    screen: Screen,
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
    dir_files: usize,
    dir_bytes: u64,
    cur_file: String,
    files_done: usize,
    files_total: usize,
    started_at: Option<std::time::Instant>,
    finished_at: Option<std::time::Instant>,
    log: String,
    rx: Option<Receiver<Event>>,
    pick_rx: Option<Receiver<Option<PathBuf>>>,
    cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    pipeline: usize,
    show_about: bool,
    prescanning: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            mode: Mode::Receive,
            screen: Screen::Home,
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
            dir_files: 0,
            dir_bytes: 0,
            cur_file: String::new(),
            files_done: 0,
            files_total: 0,
            started_at: None,
            finished_at: None,
            log: String::new(),
            rx: None,
            pick_rx: None,
            cancel: None,
            pipeline: 8,
            show_about: false,
            prescanning: false,
        }
    }
}

fn fmt_duration(secs: u64) -> String {
    format!("{} min {:02} s", secs / 60, secs % 60)
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

    fn prescan_dir(&mut self) {
        let path = PathBuf::from(&self.dir);
        if !path.is_dir() {
            self.dir_files = 0;
            self.dir_bytes = 0;
            self.prescanning = false;
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.dir_files = 0;
        self.dir_bytes = 0;
        self.prescanning = true;
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let res = crate::protocol::scan_dir(&path, |done, total, current| {
                let _ = tx.send(Event::Scan {
                    done,
                    total,
                    current: current.to_string(),
                });
            });
            match res {
                Ok(files) => {
                    let bytes = files.iter().map(|f| f.size).sum();
                    let _ = tx.send(Event::DirInfo {
                        files: files.len(),
                        bytes,
                    });
                }
                Err(e) => {
                    let _ = tx.send(Event::Log(format!("[analyse] {e}")));
                }
            }
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
        self.cur_file = String::new();
        self.files_done = 0;
        self.files_total = 0;
        self.started_at = Some(std::time::Instant::now());
        self.finished_at = None;
        self.prescanning = false;
        self.log.clear();
        self.screen = Screen::Running;

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

    fn reset_to_home(&mut self) {
        self.screen = Screen::Home;
        self.running = false;
        self.finished = None;
        self.error = None;
        self.rx = None;
        self.cancel = None;
        self.sent = 0;
        self.total = 0;
        self.pct = 0;
        self.speed_mbps = 0.0;
        self.cur_file.clear();
        self.files_done = 0;
        self.files_total = 0;
        self.scanning = false;
        self.log.clear();
    }

    fn open_in_explorer(path: &str) {
        if path.is_empty() {
            return;
        }
        #[cfg(target_os = "windows")]
        let res = std::process::Command::new("explorer").arg(path).spawn();
        #[cfg(target_os = "macos")]
        let res = std::process::Command::new("open").arg(path).spawn();
        #[cfg(all(unix, not(target_os = "macos")))]
        let res = std::process::Command::new("xdg-open").arg(path).spawn();
        let _ = res.map(|_| ());
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
                Ok(Event::File { rel, done, total }) => {
                    self.cur_file = rel;
                    self.files_done = done;
                    self.files_total = total;
                }
                Ok(Event::DirInfo { files, bytes }) => {
                    self.dir_files = files;
                    self.dir_bytes = bytes;
                    self.scanning = false;
                    self.prescanning = false;
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
                    self.prescanning = false;
                    self.finished_at = Some(std::time::Instant::now());
                    self.finished = Some((ok, files_ok, files_total));
                    self.screen = if ok {
                        Screen::Finished
                    } else {
                        Screen::Interrupted
                    };
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    if self.prescanning {
                        self.prescanning = false;
                        self.scanning = false;
                    }
                    if !self.running {
                        self.rx = None;
                    }
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
                    if self.mode == Mode::Send {
                        self.prescan_dir();
                    }
                }
                Ok(None) => self.pick_rx = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(100));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.pick_rx = None,
            }
        }
        if self.running || self.scanning || self.prescanning {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }

        egui::Panel::bottom("rmx_footer")
            .frame(
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .corner_radius(egui::CornerRadius::same(10))
                    .fill(egui::Color32::TRANSPARENT),
            )
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new("IkwnaW5mb3JtYXRpb24sIGMnZXN0IGxlIHBvdXZvaXIuIE1haXMgY29tbWUgdG91dCBwb3V2b2lyLCBpbCB5IGEgY2V1eCBxdWkgdmV1bGVudCBsZSBnYXJkZXIgcG91ciBldXguIiAtIEFhcm9uIFN3YXJ0eg==").monospace().color(OMBRE));
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::same(20)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.screen {
                        Screen::Home => self.ui_home(ui),
                        Screen::Prepare => self.ui_prepare(ui),
                        Screen::Running => self.ui_running(ui),
                        Screen::Finished => self.ui_finished(ui),
                        Screen::Interrupted => self.ui_interrupted(ui),
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

impl App {
    fn ui_header(&mut self, ui: &mut egui::Ui, title: &str, subtitle: &str) {
        ui.horizontal(|ui| {
            ui.heading("DirTransfert");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(egui::RichText::new("À propos").color(SPECTRE))
                    .clicked()
                {
                    self.show_about = true;
                }
            });
        });
        ui.add_space(6.0);
        ui.label(egui::RichText::new(title).size(22.0).color(ENCRE).strong());
        ui.label(egui::RichText::new(subtitle).color(BRUME));
        ui.add_space(10.0);
    }

    fn ui_home(&mut self, ui: &mut egui::Ui) {
        self.ui_header(
            ui,
            "Transfert direct PC à PC, sans serveur",
            "Des milliers de fichiers, un seul dossier. Choisissez le rôle de ce PC pour commencer.",
        );

        ui.columns(2, |cols| {
            for (i, col) in cols.iter_mut().enumerate() {
                card_frame().show(col, |ui| {
                    let (title, desc) = if i == 0 {
                        (
                            "Envoyer",
                            "Choisissez un dossier et connectez-vous au PC qui le reçoit.",
                        )
                    } else {
                        (
                            "Recevoir",
                            "Choisissez où enregistrer les fichiers et partagez votre code.",
                        )
                    };
                    ui.label(egui::RichText::new(title).size(19.0).color(ENCRE).strong());
                    ui.label(egui::RichText::new(desc).color(BRUME));
                    ui.add_space(8.0);
                    if ui
                        .button(egui::RichText::new("Choisir ce rôle").color(SPECTRE))
                        .clicked()
                    {
                        self.mode = if i == 0 { Mode::Send } else { Mode::Receive };
                        self.screen = Screen::Prepare;
                    }
                });
            }
        });

        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(
                "Sur le même réseau ou à distance : les fichiers passent directement entre vos deux PC, sans serveur tiers.",
            )
            .color(BRUME),
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("ChaCha20-Poly1305 · chiffrement").color(TERMINAL));
            ui.label(egui::RichText::new("SHA-256 · intégrité").color(TERMINAL));
        });
        ui.label(
            egui::RichText::new(
                "Le chiffrement protège le contenu de bout en bout. L'intégrité vérifie que les fichiers reçus sont identiques.",
            )
            .color(OMBRE),
        );
    }

    fn ui_code_boxes(&mut self, ui: &mut egui::Ui, enabled: bool) {
        let clean: String = self
            .code
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        ui.horizontal(|ui| {
            for (i, ch) in clean.chars().take(crate::crypto::CODE_LEN).enumerate() {
                if i == 4 {
                    ui.label(egui::RichText::new("-").size(24.0).color(OMBRE));
                }
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(0x18, 0x18, 0x18))
                    .corner_radius(RMX_ROUNDING)
                    .stroke(egui::Stroke::new(RMX_STROKE, SPECTRE))
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(ch.to_string()).size(20.0).color(ENCRE));
                    });
            }
            let _ = enabled;
        });
    }

    fn ui_prepare(&mut self, ui: &mut egui::Ui) {
        let enabled = !self.running;
        match self.mode {
            Mode::Send => self.ui_header(
                ui,
                "Envoyer un dossier",
                "Renseignez les informations partagées par le PC qui reçoit.",
            ),
            Mode::Receive => self.ui_header(
                ui,
                "Recevoir un dossier",
                "Préparez ce PC, puis partagez une adresse et le code à l'expéditeur.",
            ),
        }

        ui.horizontal(|ui| {
            if ui
                .button(egui::RichText::new("← Accueil").color(SPECTRE))
                .clicked()
            {
                self.screen = Screen::Home;
                self.rx = None;
                self.prescanning = false;
                self.scanning = false;
            }
        });
        ui.add_space(8.0);

        card_frame().show(ui, |ui| {
            if self.mode == Mode::Send {
                ui.label(egui::RichText::new("Dossier à envoyer").color(SPECTRE));
                field_frame().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_enabled(
                            enabled,
                            egui::TextEdit::singleline(&mut self.dir)
                                .desired_width(f32::INFINITY)
                                .hint_text("C:\\..."),
                        );
                        if ui
                            .add_enabled(
                                enabled && self.pick_rx.is_none(),
                                egui::Button::new("Choisir un dossier"),
                            )
                            .clicked()
                        {
                            self.browse();
                        }
                    });
                });
                if self.prescanning || self.scanning {
                    ui.label(
                        egui::RichText::new(format!(
                            "Analyse : {}/{} — {}",
                            self.scan_done, self.scan_total, self.scan_current
                        ))
                        .color(BRUME),
                    );
                } else if self.dir_files > 0 {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} fichiers · {} · sous-dossiers inclus",
                            self.dir_files,
                            transfer::human(self.dir_bytes)
                        ))
                        .color(TERMINAL),
                    );
                }
                ui.add_space(10.0);

                ui.label(
                    egui::RichText::new("Adresse du PC destinataire (IP:port)").color(SPECTRE),
                );
                field_frame().show(ui, |ui| {
                    ui.add_enabled(
                        enabled,
                        egui::TextEdit::singleline(&mut self.addr)
                            .desired_width(f32::INFINITY)
                            .hint_text("192.0.2.24:45822"),
                    );
                });
                ui.label(
                    egui::RichText::new(
                        "Sur le même réseau, utilisez l'adresse locale du PC qui reçoit. Sinon, son adresse publique.",
                    )
                    .color(OMBRE),
                );
                ui.add_space(10.0);
            } else {
                ui.label(egui::RichText::new("Enregistrer les fichiers dans").color(SPECTRE));
                field_frame().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_enabled(
                            enabled,
                            egui::TextEdit::singleline(&mut self.out_dir)
                                .desired_width(f32::INFINITY)
                                .hint_text("D:\\Transferts"),
                        );
                        if ui
                            .add_enabled(
                                enabled && self.pick_rx.is_none(),
                                egui::Button::new("Choisir un dossier"),
                            )
                            .clicked()
                        {
                            self.browse();
                        }
                    });
                });
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(enabled, egui::Button::new("Générer"))
                        .clicked()
                    {
                        self.code = crate::crypto::gen_code();
                    }
                    if ui.add_enabled(enabled, egui::Button::new("Copier")).clicked() {
                        ui.ctx().copy_text(
                            crate::crypto::norm_code(&self.code)
                                .unwrap_or_else(|| self.code.clone()),
                        );
                    }
                });
                ui.checkbox(&mut self.upnp, "Ouvrir la redirection de port (UPnP)");
                ui.checkbox(&mut self.open_fw, "Ouvrir le pare-feu Windows (administrateur)");
                ui.checkbox(&mut self.close_fw, "Refermer pare-feu + UPnP après le transfert");
                ui.add_enabled(
                    enabled,
                    egui::Slider::new(&mut self.pipeline, 1..=64).text("requêtes en vol"),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("En attente de connexion...")
                        .color(ETRANGE),
                );
                ui.label(
                    egui::RichText::new(
                        "Même réseau : adresse locale. À distance : adresse publique et port à ouvrir sur le routeur (UPnP ou redirection).",
                    )
                    .color(OMBRE),
                );
                ui.label(
                    egui::RichText::new(
                        "CGNAT : la connexion distante peut être bloquée. Windows peut demander les droits administrateur pour le pare-feu.",
                    )
                    .color(OMBRE),
                );
            }

            ui.add_space(10.0);
            ui.label(egui::RichText::new("Code d'appairage").color(SPECTRE));
            self.ui_code_boxes(ui, enabled);
            ui.horizontal(|ui| {
                ui.add_enabled(
                    enabled,
                    egui::TextEdit::singleline(&mut self.code)
                        .desired_width(160.0)
                        .hint_text("ABCD-2345"),
                        )
                        ;
                if ui
                    .add_enabled(enabled, egui::Button::new("Copier le code"))
                    .clicked()
                {
                    ui.ctx().copy_text(
                        crate::crypto::norm_code(&self.code)
                            .unwrap_or_else(|| self.code.clone()),
                    );
                }
                if self.mode == Mode::Receive
                    && ui
                        .add_enabled(enabled, egui::Button::new("Générer"))
                        .clicked()
                {
                    self.code = crate::crypto::gen_code();
                }
            });
            ui.label(
                egui::RichText::new(
                    "Recopiez le code affiché sur l'autre PC pour associer les deux appareils.",
                )
                .color(OMBRE),
            );
            self.code_error = match crate::crypto::norm_code(&self.code) {
                Some(_) => None,
                None if self.code.trim().is_empty() => None,
                None => Some(
                    "code invalide : 8 caractères sans O/0/1/I (ex. ABCD-2345)".to_string(),
                ),
            };
            if let Some(e) = &self.code_error {
                ui.colored_label(egui::Color32::from_rgb(0xFF, 0x6A, 0x4D), e);
            }
            ui.label(
                egui::RichText::new("Chiffrement de bout en bout · intégrité vérifiée à la réception")
                    .color(BRUME),
            );
            ui.label(
                egui::RichText::new("ChaCha20-Poly1305 · chiffrement   SHA-256 · intégrité")
                    .color(TERMINAL),
            );
        });

        ui.add_space(12.0);
        let can_start = enabled
            && crate::crypto::norm_code(&self.code).is_some()
            && match self.mode {
                Mode::Send => {
                    !self.dir.trim().is_empty() && transfer::parse_endpoint(&self.addr).is_some()
                }
                Mode::Receive => {
                    !self.out_dir.trim().is_empty()
                        && transfer::parse_endpoint(&self.addr).is_some()
                }
            };
        let start_label = if self.mode == Mode::Send {
            "Démarrer le transfert"
        } else {
            "Démarrer la réception"
        };
        if ui
            .add_enabled(
                can_start,
                egui::Button::new(egui::RichText::new(start_label).color(SPECTRE).size(16.0)),
            )
            .clicked()
        {
            self.start_transfer();
        }
    }

    fn ui_running(&mut self, ui: &mut egui::Ui) {
        let title = match self.mode {
            Mode::Send => format!("Transfert en cours — {} → {}", self.dir, self.addr),
            Mode::Receive => format!("Réception en cours — écoute sur {}", self.addr),
        };
        self.ui_header(ui, "Transfert en cours", &title);

        card_frame().show(ui, |ui| {
            if self.scanning {
                let frac = if self.scan_total > 0 {
                    self.scan_done as f32 / self.scan_total as f32
                } else {
                    0.0
                };
                ui.add(
                    egui::ProgressBar::new(frac).text(format!(
                        "Analyse : {}/{} fichiers — {}",
                        self.scan_done, self.scan_total, self.scan_current
                    )),
                );
            }
            let frac = if self.total > 0 {
                self.pct as f32 / 100.0
            } else {
                0.0
            };
            let bar = egui::ProgressBar::new(frac)
                .text(format!(
                    "{} / {} — {:.1} Mo/s",
                    transfer::human(self.sent),
                    transfer::human(self.total),
                    self.speed_mbps
                ))
                .desired_height(22.0);
            ui.add(bar);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{} %", self.pct))
                        .size(26.0)
                        .color(ENCRE)
                        .strong(),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "{} / {}",
                        transfer::human(self.sent),
                        transfer::human(self.total)
                    ))
                    .color(BRUME),
                );
                ui.label(egui::RichText::new(format!("{:.1} Mo/s", self.speed_mbps)).color(BRUME));
                if self.files_total > 0 {
                    ui.label(
                        egui::RichText::new(format!("{}/{} fichiers", self.files_done, self.files_total))
                            .color(BRUME),
                    );
                }
            });
            if !self.cur_file.is_empty() {
                ui.label(egui::RichText::new("Fichier en cours").color(SPECTRE));
                ui.label(egui::RichText::new(&self.cur_file).monospace().color(ENCRE));
            }
            ui.label(
                egui::RichText::new(
                    "Transfert interrompu ? Relancez : seuls les fichiers manquants seront retransférés.",
                )
                .color(OMBRE),
            );
        });

        ui.add_space(12.0);
        ui.label(egui::RichText::new("JOURNAL DU TRANSFERT").color(SPECTRE));
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
                                .desired_rows(10)
                                .interactive(false),
                        );
                    });
            });

        ui.add_space(8.0);
        if ui
            .button(egui::RichText::new("Annuler").color(ETRANGE))
            .clicked()
        {
            if let Some(c) = &self.cancel {
                c.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            self.log.push_str("annulation demandée...\n");
        }
    }

    fn ui_finished(&mut self, ui: &mut egui::Ui) {
        let (_, files_ok, files_total) = self.finished.unwrap_or((true, 0, 0));
        let duration = match (self.started_at, self.finished_at) {
            (Some(a), Some(b)) => fmt_duration(b.duration_since(a).as_secs()),
            _ => String::new(),
        };
        self.ui_header(
            ui,
            "Transfert terminé",
            "Votre dossier est arrivé. Tous les fichiers ont été reçus et vérifiés.",
        );

        card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(files_ok.to_string())
                            .size(28.0)
                            .color(ENCRE)
                            .strong(),
                    );
                    ui.label(egui::RichText::new("fichiers reçus").color(BRUME));
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(transfer::human(self.total))
                            .size(28.0)
                            .color(ENCRE)
                            .strong(),
                    );
                    ui.label(egui::RichText::new("taille totale").color(BRUME));
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(duration)
                            .size(28.0)
                            .color(ENCRE)
                            .strong(),
                    );
                    ui.label(egui::RichText::new("durée du transfert").color(BRUME));
                });
            });
            let _ = files_total;
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Dossier enregistré").color(SPECTRE));
            let dest = if self.mode == Mode::Send {
                self.addr.clone()
            } else {
                self.out_dir.clone()
            };
            ui.label(egui::RichText::new(&dest).monospace().color(ENCRE));
            ui.label(egui::RichText::new("SHA-256 vérifié").color(TERMINAL));
            ui.label(
                egui::RichText::new("ChaCha20-Poly1305 · chiffrement   SHA-256 · intégrité")
                    .color(TERMINAL),
            );
        });

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui
                .button(egui::RichText::new("Retour à l'accueil").color(SPECTRE))
                .clicked()
            {
                self.reset_to_home();
            }
            if self.mode == Mode::Receive && ui.button("Ouvrir le dossier").clicked() {
                let dir = self.out_dir.clone();
                Self::open_in_explorer(&dir);
            }
        });
    }

    fn ui_interrupted(&mut self, ui: &mut egui::Ui) {
        self.ui_header(
            ui,
            "Connexion interrompue",
            "Le transfert a été arrêté. Les fichiers déjà reçus sont conservés.",
        );

        card_frame().show(ui, |ui| {
            ui.label(egui::RichText::new("Reprise").color(SPECTRE));
            ui.label(
                egui::RichText::new(
                    "Transfert interrompu ? Relancez : seuls les fichiers manquants seront retransférés.",
                )
                .color(BRUME),
            );
            ui.add_space(6.0);
            if let Some(e) = &self.error {
                ui.colored_label(egui::Color32::from_rgb(0xFF, 0x6A, 0x4D), e);
            }
        });

        ui.add_space(8.0);
        ui.label(egui::RichText::new("JOURNAL DU TRANSFERT").color(SPECTRE));
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
                                .desired_rows(8)
                                .interactive(false),
                        );
                    });
            });

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui
                .button(egui::RichText::new("Modifier la connexion").color(SPECTRE))
                .clicked()
            {
                self.screen = Screen::Prepare;
                self.running = false;
                self.finished = None;
                self.rx = None;
                self.cancel = None;
            }
            if ui
                .button(egui::RichText::new("Réessayer").color(TERMINAL))
                .clicked()
            {
                self.start_transfer();
            }
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
    let icon = egui::IconData {
        rgba: ICON_RGBA.to_vec(),
        width: ICON_W,
        height: ICON_H,
    };
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([720.0, 640.0])
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
        let log_enabled = std::env::var_os("DIRTRANSFER_LOG").is_some();
        eframe::run_native(
            "DirTransfert",
            native,
            Box::new(move |cc| {
                use std::io::Write;
                if log_enabled {
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
                }
                cc.egui_ctx.set_visuals(rmx_visuals());
                Ok(Box::new(App::default()))
            }),
        )
        .map_err(|e| format!("lancement de la fenetre (renderer {:?}) : {}", renderer, e))
    };

    let log_step = |msg: String| {
        if std::env::var_os("DIRTRANSFER_LOG").is_none() {
            return;
        }
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
    let force_glow = std::env::var_os("DIRTRANSFER_FORCE_GLOW").is_some();
    let result = if force_glow {
        log_step("--render=glow : glow force, wgpu ignore".to_string());
        Err("glow force par --render=glow".to_string())
    } else {
        log_step("tentative renderer wgpu".to_string());
        run(eframe::Renderer::Wgpu)
    };
    if result.is_ok() {
        log_step("wgpu : fermeture normale".to_string());
        return Ok(());
    }
    log_step("renderer wgpu echoue, tentative glow (OpenGL)".to_string());
    match run(eframe::Renderer::Glow) {
        Err(glow_err) => {
            let msg = format!("wgpu : {}\nglow : {}", result.unwrap_err(), glow_err);
            log_step(format!("echec de tous les renderers\n{}", msg));
            Err(msg)
        }
        Ok(()) => {
            log_step("glow : fermeture normale".to_string());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_app_state_coherent() {
        let a = App::default();
        assert_eq!(a.mode, Mode::Receive);
        assert_eq!(a.screen, Screen::Home);
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

    #[test]
    fn duree_formatee() {
        assert_eq!(fmt_duration(238), "3 min 58 s");
        assert_eq!(fmt_duration(0), "0 min 00 s");
    }
}

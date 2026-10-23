//! Slint-based GUI for DirTransfert
//! Migration from egui to Slint

use dirtransfer::crypto;
use dirtransfer::transfer::{self, Opts, Reporter};
use rfd::FileDialog;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

slint::slint! {
    import { Button, CheckBox, LineEdit, ScrollView, Slider } from "std-widgets.slint";

    // ---- Charte graphique R_M_X ----
    // NEANT #111111, SURFACE #222222, ENCRE #FFFFFF, BRUME #CCCCCC
    // SPECTRE #00FFFF, ETRANGE #FF00FF, TERMINAL #00FF7F, OMBRE #555555
    global RmxTheme {
        out property <color> neant: #111111;
        out property <color> surface: #222222;
        out property <color> encre: #FFFFFF;
        out property <color> brume: #CCCCCC;
        out property <color> spectre: #00FFFF;
        out property <color> etrange: #FF00FF;
        out property <color> terminal: #00FF7F;
        out property <color> ombre: #555555;
        out property <length> radius: 10px;
    }

    component RmxLabel inherits Rectangle {
        in property <string> text;
        in property <length> min-label-width: 90px;
        in property <bool> highlight: false;
        in property <TextHorizontalAlignment> h-alignment: left;
        height: 30px;
        min-width: min-label-width;
        Rectangle {
            width: parent.width;
            Text {
                width: parent.width;
                text: root.text;
                color: root.highlight ? RmxTheme.spectre : RmxTheme.brume;
                font-size: 14px;
                vertical-alignment: center;
                horizontal-alignment: root.h-alignment;
                overflow: elide;
            }
        }
    }

    component RmxValue inherits Rectangle {
        in property <string> text;
        background: RmxTheme.surface;
        border-radius: RmxTheme.radius;
        height: 32px;
        min-width: 120px;
        Rectangle {
            width: parent.width;
            Text {
                width: parent.width;
                text: root.text;
                color: RmxTheme.terminal;
                font-family: "monospace";
                font-size: 14px;
                vertical-alignment: center;
                horizontal-alignment: center;
                overflow: elide;
            }
        }
    }

    component RmxTextInput inherits Rectangle {
        in-out property <string> text;
        in property <string> placeholder;
        in property <TextHorizontalAlignment> h-alignment: left;
        callback edited(string);
        background: RmxTheme.surface;
        border-radius: RmxTheme.radius;
        height: 32px;
        min-width: 120px;
        forward-focus: input;
        TextInput {
            input := TextInput {
                width: parent.width;
                text <=> root.text;
                color: RmxTheme.encre;
                font-family: "monospace";
                font-size: 14px;
                horizontal-alignment: root.h-alignment;
                vertical-alignment: center;
                edited => { root.edited(self.text); }
            }
        }
        if (input.text == "" && !input.has-focus) : Text {
            x: 10px;
            width: parent.width - 20px;
            text: root.placeholder;
            color: RmxTheme.ombre;
            font-family: "monospace";
            font-size: 14px;
            horizontal-alignment: root.h-alignment;
            vertical-alignment: center;
        }
    }

    component RmxButton inherits Rectangle {
        in property <string> text;
        in property <bool> accent: false;
        in property <bool> primary: false;
        in property <bool> enabled: true;
        in property <int> font-weight: 400;
        in property <length> font-size: 14px;
        in property <length> min-button-width: 90px;
        callback clicked();
        property <bool> hovered;
        property <bool> pressed;
        property <bool> down: pressed && hovered;
        background: !root.enabled ? RmxTheme.surface
            : root.primary ? (root.down ? #00cccc : (root.hovered ? #33ffff : RmxTheme.spectre))
            : (root.down ? RmxTheme.surface : (root.hovered ? #2a2a2a : RmxTheme.surface));
        border-radius: RmxTheme.radius;
        border-width: root.primary || root.accent ? 2px : 1px;
        height: 34px;
        min-width: root.min-button-width;
        border-color: root.primary ? RmxTheme.spectre : (root.accent ? RmxTheme.etrange : RmxTheme.ombre);
        label := Text {
            text: root.text;
            color: !root.enabled ? RmxTheme.ombre : (root.primary ? RmxTheme.neant : RmxTheme.encre);
            font-weight: root.font-weight;
            font-size: root.font-size;
            horizontal-alignment: center;
            vertical-alignment: center;
        }
        touch := TouchArea {
            visible: root.enabled;
            mouse-cursor: pointer;
            clicked => { root.clicked(); }
            changed has-hover => { root.hovered = self.has-hover; }
            pointer-event => {
                root.pressed = self.pressed;
            }
        }
    }

    component RmxCheckBox inherits Rectangle {
        in property <string> text;
        in-out property <bool> checked;
        callback toggled();
        property <bool> hovered;
        border-radius: RmxTheme.radius;
        height: 28px;
        min-width: 120px;
        HorizontalLayout {
            padding-left: 2px;
            spacing: 8px;
            Rectangle {
                box := Rectangle {
                    y: (parent.height - self.height) / 2;
                    width: 18px;
                    height: 18px;
                    border-radius: 5px;
                    border-width: 1px;
                    border-color: root.checked ? RmxTheme.spectre : RmxTheme.ombre;
                    background: root.checked ? RmxTheme.spectre : RmxTheme.neant;
                    if root.checked: Text {
                        text: "×";
                        color: RmxTheme.neant;
                        font-size: 14px;
                        horizontal-alignment: center;
                        vertical-alignment: center;
                    }
                }
                TouchArea {
                    mouse-cursor: pointer;
                    clicked => {
                        root.checked = !root.checked;
                        root.toggled();
                    }
                    changed has-hover => {
                        root.hovered = self.has-hover;
                    }
                }
            }
            Text {
                text: root.text;
                color: RmxTheme.brume;
                font-size: 14px;
                vertical-alignment: center;
                overflow: elide;
            }
        }
    }

    component RmxSlider inherits Rectangle {
        in-out property <float> value;
        callback changed(float);
        height: 28px;
        HorizontalLayout {
            Slider {
                value <=> root.value;
                changed(value) => { root.changed(self.value); }
            }
        }
    }


    export component AppWindow inherits Window {
        background: RmxTheme.neant;
        
        in-out property <string> mode: "receive";
        in-out property <bool> mode-send: false;
        in-out property <string> code;
        in-out property <string> dir;
        in-out property <string> addr: "0.0.0.0:47500";
        in-out property <string> out-dir;
        in-out property <bool> upnp: true;
        in-out property <bool> open-fw: true;
        in-out property <bool> close-fw: true;
        in-out property <bool> prefer-ipv6: false;
        in-out property <int> pipeline: 8;
        in-out property <float> pipeline-float: 8;
        in-out property <bool> running: false;
        in-out property <string> log-text;
        in-out property <string> pipeline-text: "8";
        in-out property <string> hidden: "Um1YIHdvcmsgaW4gc2hhZG93cywgYXVkaW8gaW4gdGhlIHdpcmVzLgo=";

        callback mode-set(string);
        callback browse-dir();
        callback browse-out-dir();
        callback generate-code();
        callback start-transfer();
        callback cancel-transfer();
        callback dir-edited(string);
        
        min-width: 680px;
        min-height: 640px;
        preferred-width: 820px;
        preferred-height: 660px;
        
        VerticalLayout {
            padding: 16px;
            spacing: 12px;
                
                // Header
                Rectangle {
                    height: 44px;
                    Text {
                        text: "DirTransfert";
                        color: RmxTheme.encre;
                        font-size: 22px;
                        font-weight: 700;
                        horizontal-alignment: center;
                    }
                    Rectangle {
                        y: 34px;
                        height: 1px;
                        background: RmxTheme.ombre;
                    }
                }
                
                // Mode selection
                Rectangle {
                    height: 36px;
                    HorizontalLayout {
                        spacing: 10px;
                        RmxLabel { text: "Mode :"; }
                        RmxButton {
                            text: "Envoyer";
                            accent: root.mode == "send";
                            clicked => { mode-set("send"); }
                        }
                        RmxButton {
                            text: "Recevoir";
                            accent: root.mode == "receive";
                            clicked => { mode-set("receive"); }
                        }
                        Rectangle { }
                    }
                }
                
                // Code section
                Rectangle {
                    height: 36px;
                    HorizontalLayout {
                        spacing: 10px;
                        RmxLabel { text: "Code :"; }
                        code-input := RmxTextInput {
                            text <=> root.code;
                            placeholder: "XXXX-XXXX";
                            h-alignment: center;
                        }
                        RmxButton {
                            text: "Générer";
                            clicked => { root.generate-code(); }
                        }
                        Rectangle { }
                    }
                }
                
                // Directory section
                Rectangle {
                    height: 36px;
                    HorizontalLayout {
                        spacing: 10px;
                        RmxLabel { text: root.mode == "send" ? "Répertoire :" : "Réception :"; min-label-width: 110px; }
                        RmxTextInput {
                            text: root.mode == "send" ? root.dir : root.out-dir;
                            placeholder: "/chemin/vers/repertoire";
                            edited(text) => { root.dir-edited(text); }
                        }
                        RmxButton {
                            text: "Parcourir";
                            clicked => { root.mode == "send" ? root.browse-dir() : root.browse-out-dir(); }
                        }
                        Rectangle { }
                    }
                }
                
                // Address section
                Rectangle {
                    height: 36px;
                    HorizontalLayout {
                        spacing: 10px;
                        RmxLabel { text: "Adresse :"; }
                        RmxTextInput {
                            text <=> root.addr;
                            placeholder: "hote:port (ex. 192.168.1.20:47500)";
                            h-alignment: center;
                        }
                        Rectangle { }
                    }
                }
                
                // Options
                Rectangle {
                    height: 36px;
                    HorizontalLayout {
                        spacing: 20px;
                        RmxCheckBox {
                            text: "UPnP";
                            checked <=> root.upnp;
                        }
                        RmxCheckBox {
                            text: "Pare-feu";
                            checked <=> root.open-fw;
                        }
                        RmxCheckBox {
                            text: "IPv6";
                            checked <=> root.prefer-ipv6;
                        }
                        RmxCheckBox {
                            text: "Fermer après";
                            checked <=> root.close-fw;
                        }
                        Rectangle { }
                    }
                }
                
                // Pipeline
                Rectangle {
                    height: 36px;
                    HorizontalLayout {
                        spacing: 10px;
                        RmxLabel { text: "Pipeline :"; }
                        RmxSlider {
                            value <=> root.pipeline-float;
                            changed(value) => {
                                root.pipeline-text = "N=" + floor(value);
                                root.pipeline = round(value);
                            }
                        }
                        RmxValue { text: root.pipeline-text; min-width: 60px; }
                        Rectangle { }
                    }
                }
                
                // Action buttons
                Rectangle {
                    height: 44px;
                    HorizontalLayout {
                        spacing: 10px;
                        RmxButton {
                            text: "Démarrer";
                            primary: true;
                            enabled: !root.running;
                            clicked => { root.start-transfer(); }
                        }
                        RmxButton {
                            text: "Annuler";
                            accent: true;
                            enabled: root.running;
                            clicked => { root.cancel-transfer(); }
                        }
                        Rectangle { }
                    }
                }
                
                // Log area
                Rectangle {
                    background: RmxTheme.surface;
                    border-radius: RmxTheme.radius;
                    vertical-stretch: 1;
                    min-height: 160px;
                    log-scroll := ScrollView {
                        x: 5px;
                        y: 5px;
                        width: parent.width - 10px;
                        height: parent.height - 10px;
                        content-height: max(self.visible-height, log-input.preferred-height);
                        log-input := TextInput {
                            read-only: true;
                            single-line: false;
                            wrap: word-wrap;
                            text: root.log-text;
                            color: RmxTheme.terminal;
                            font-family: "monospace";
                            font-size: 12px;
                        }
                    }
                }
                
                // Footer / hidden layer
                Rectangle {
                    height: 18px;
                    Text {
                        text: root.hidden;
                        color: #2a2a2a;
                        font-family: "monospace";
                        font-size: 9px;
                        horizontal-alignment: center;
                    }
        // La couche cachée ne s'explique pas, elle se cherche.
                    Rectangle {
                        y: 16px;
                        height: 1px;
                        background: #2a2a2a;
                    }
                }
        }
    }
}

// Event for communication between threads
#[derive(Debug, Clone)]
enum UiEvent {
    Log(String),
    BrowseDir(String),
    BrowseOutDir(String),
    #[allow(dead_code)]
    Cancel,
    SetRunning(bool),
    SetCode(String),
}

// GUI Reporter
struct SlintReporter {
    tx: Sender<UiEvent>,
}

impl Reporter for SlintReporter {
    fn log(&self, msg: String) {
        let _ = self.tx.send(UiEvent::Log(msg));
    }

    fn scan(&self, done: usize, total: usize, current: &str) {
        let _ = self.tx.send(UiEvent::Log(format!(
            "Analyse: {}/{}: {}",
            done, total, current
        )));
    }

    fn progress(&self, sent: u64, total: u64, speed_mbps: f64, pct: u64) {
        let _ = self.tx.send(UiEvent::Log(format!(
            "Progress: {}/{} ({:.1} Mo/s, {}%)",
            sent, total, speed_mbps, pct
        )));
    }

    fn done(&self, ok: bool, files_ok: usize, files_total: usize) {
        if ok {
            let _ = self.tx.send(UiEvent::Log(format!(
                "[ok] {}/{} fichiers valides",
                files_ok, files_total
            )));
        } else {
            let _ = self.tx.send(UiEvent::Log(format!(
                "[erreur] Transfert incomplet ({}/{} ok)",
                files_ok, files_total
            )));
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;

    // Set initial values
    ui.set_code(crypto::gen_code().into());
    ui.set_pipeline_text("N=8".into());

    // Create channel for events
    let (tx, rx) = channel();

    // Store receiver in Rc<RefCell> so we can access it from callbacks
    let rx_ref = Rc::new(RefCell::new(rx));

    // Shared cancel flag
    let cancel_flag = Arc::new(AtomicBool::new(false));

    // Set up callbacks
    let tx_clone = tx.clone();
    ui.on_browse_dir(move || {
        let tx = tx_clone.clone();
        std::thread::spawn(move || {
            if let Some(path) = FileDialog::new().pick_folder() {
                let path_str = path.display().to_string();
                let _ = tx.send(UiEvent::BrowseDir(path_str));
            }
        });
    });

    let tx_clone = tx.clone();
    ui.on_browse_out_dir(move || {
        let tx = tx_clone.clone();
        std::thread::spawn(move || {
            if let Some(path) = FileDialog::new().pick_folder() {
                let path_str = path.display().to_string();
                let _ = tx.send(UiEvent::BrowseOutDir(path_str));
            }
        });
    });

    let tx_clone = tx.clone();
    ui.on_generate_code(move || {
        let code = crypto::gen_code();
        let _ = tx_clone.send(UiEvent::SetCode(code));
    });

    // Mode switching via dedicated callback
    const DEFAULT_LISTEN_ADDR: &str = "0.0.0.0:47500";
    let ui_handle = ui.as_weak();
    ui.on_mode_set(move |mode| {
        if let Some(ui) = ui_handle.upgrade() {
            let prev: String = ui.get_mode().into();
            let mode_str: String = mode.to_string();
            if prev != mode_str {
                // Restaure l'adresse d'ecoute par defaut quand on repasse en reception,
                // a moins que l'utilisateur n'ait saisi une adresse personnalisee en mode envoi.
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
            }
        }
    });


    // Directory text edited: sync dir or out-dir depending on mode
    let ui_handle = ui.as_weak();
    ui.on_dir_edited(move |text| {
        if let Some(ui) = ui_handle.upgrade() {
            let mode: String = ui.get_mode().into();
            if mode == "send" {
                ui.set_dir(text.clone());
            } else {
                ui.set_out_dir(text.clone());
            }
        }
    });

    let tx_clone = tx.clone();
    let ui_handle = ui.as_weak();
    let cancel_handle = cancel_flag.clone();
    ui.on_start_transfer(move || {
        let tx = tx_clone.clone();
        let _ = tx.send(UiEvent::Log("Bouton Demarrer clique".into()));
        if let Some(ui) = ui_handle.upgrade() {
            // Reset cancel flag
            cancel_handle.store(false, Ordering::SeqCst);
            start_transfer(&ui, tx, cancel_handle.clone());
        }
    });

    let tx_clone = tx.clone();
    let cancel_handle = cancel_flag.clone();
    ui.on_cancel_transfer(move || {
        cancel_handle.store(true, Ordering::SeqCst);
        let _ = tx_clone.send(UiEvent::Log("Annulation demandee...".into()));
    });

    // Set up timer to process events
    let ui_handle = ui.as_weak();
    let rx_ref_timer = rx_ref.clone();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(50),
        move || {
            let rx = rx_ref_timer.borrow_mut();
            while let Ok(event) = rx.try_recv() {
                if let Some(ui) = ui_handle.upgrade() {
                    process_event(&ui, event);
                }
            }
        },
    );

    // Run the event loop
    ui.run()?;

    Ok(())
}

fn process_event(ui: &AppWindow, event: UiEvent) {
    match event {
        UiEvent::Log(msg) => {
            let current_log: String = ui.get_log_text().into();
            let new_log = format!("{}{}\n", current_log, msg);
            ui.set_log_text(new_log.into());
        }
        UiEvent::BrowseDir(path) => {
            let mode: String = ui.get_mode().into();
            if mode == "send" {
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
        }
        UiEvent::Cancel => {}
    }
}

fn start_transfer(ui: &AppWindow, tx: Sender<UiEvent>, cancel: Arc<AtomicBool>) {
    let mode_str: String = ui.get_mode().into();
    let mode_send = mode_str == "send";
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

    // Validate code
    if crypto::norm_code(&code).is_none() {
        let _ = tx.send(UiEvent::Log(
            "[erreur] Code invalide : 8 caracteres sans O/0/1/I (ex. ABCD-2345)".into(),
        ));
        return;
    }

    // Validate paths
    if mode_send && dir.is_empty() {
        let _ = tx.send(UiEvent::Log("[erreur] Repertoire a envoyer non specifie".into()));
        return;
    }

    if !mode_send && dir.is_empty() {
        let _ = tx.send(UiEvent::Log(
            "[erreur] Dossier de reception non specifie".into(),
        ));
        return;
    }

    // Validate address
    if addr.is_empty() {
        let _ = tx.send(UiEvent::Log("[erreur] Adresse non specifiee".into()));
        return;
    }

    // Validate endpoint format
    if transfer::parse_endpoint(&addr).is_none() {
        let _ = tx.send(UiEvent::Log(
            format!("[erreur] Adresse invalide : {}", addr),
        ));
        return;
    }

    // Set running state via event
    let _ = tx.send(UiEvent::SetRunning(true));
    let _ = tx.send(UiEvent::Log("Demarrage du transfert...".into()));

    let tx_thread = tx.clone();

    thread::spawn(move || {
        let rep = SlintReporter {
            tx: tx_thread.clone(),
        };

        let opts = Opts {
            upnp,
            open_fw,
            close_fw_after: close_fw,
            pipeline: Some(pipeline),
            prefer_ipv6,
            cancel: Some(cancel.clone()),
        };

        let result = if mode_send {
            let path = PathBuf::from(dir);
            transfer::send(&path, &addr, &code, &Some(cancel), &rep)
        } else {
            let path = PathBuf::from(dir);
            transfer::receive(&addr, &path, &code, &opts, &rep)
        };

        if let Err(e) = result {
            let _ = tx_thread.send(UiEvent::Log(format!("[erreur] {}", e)));
        }

        // Reset running state via event
        let _ = tx_thread.send(UiEvent::SetRunning(false));
    });
}

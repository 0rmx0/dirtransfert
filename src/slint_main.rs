//! Slint-based GUI for DirTransfert
//! Migration from egui to Slint - Working version

use dirtransfer::crypto;
use dirtransfer::transfer::{self, Opts, Reporter};
use rfd::FileDialog;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::Duration;

slint::slint! {
    import { Button, CheckBox, HorizontalBox, LineEdit, ScrollView, Slider, TextEdit, VerticalBox } from "std-widgets.slint";
    
    export component AppWindow inherits Window {
        in-out property<string> mode: "receive";
        in-out property<string> code;
        in-out property<string> dir;
        in-out property<string> addr: "0.0.0.0:47500";
        in-out property<string> out-dir;
        in-out property<bool> upnp: true;
        in-out property<bool> open-fw: true;
        in-out property<bool> close-fw: true;
        in-out property<bool> prefer-ipv6: false;
        in-out property<int> pipeline: 8;
        in-out property<bool> running: false;
        in-out property<string> log-text;
        
        min-width: 640px;
        min-height: 560px;
        preferred-width: 800px;
        preferred-height: 600px;
        background: #111111;
        
        VerticalBox {
            padding: 10px;
            spacing: 10px;
            
            HorizontalBox {
                spacing: 10px;
                TextEdit { text: "DirTransfert (Slint)"; enabled: false; font-size: 20px; }
            }
            
            HorizontalBox {
                spacing: 10px;
                TextEdit { text: "Mode:"; enabled: false; }
                Button { text: "Envoyer"; clicked => { mode = "send"; } }
                Button { text: "Recevoir"; clicked => { mode = "receive"; } }
            }
            
            HorizontalBox {
                spacing: 10px;
                TextEdit { text: "Code:"; enabled: false; }
                LineEdit { text: code; placeholder-text: "XXXX-XXXX"; }
                Button { text: "Generer"; clicked => { root.generate-code(); } }
            }
            
            HorizontalBox {
                spacing: 10px;
                TextEdit { text: "Repertoire:"; enabled: false; }
                LineEdit { text: dir; }
                Button { text: "Parcourir"; clicked => { root.browse-dir(); } }
            }
            
            HorizontalBox {
                spacing: 10px;
                TextEdit { text: "Adresse:"; enabled: false; }
                LineEdit { text: addr; }
            }
            
            CheckBox { text: "UPnP"; checked: upnp; }
            CheckBox { text: "Pare-feu"; checked: open-fw; }
            CheckBox { text: "IPv6"; checked: prefer-ipv6; }
            CheckBox { text: "Fermer apres"; checked: close-fw; }
            
            HorizontalBox {
                spacing: 10px;
                TextEdit { text: "Pipeline:"; enabled: false; }
                Slider { value: pipeline; }
            }
            
            HorizontalBox {
                spacing: 10px;
                Button { text: "Demarrer"; clicked => { root.start-transfer(); } }
                Button { text: "Annuler"; clicked => { root.cancel-transfer(); } enabled: running; }
            }
            
            ScrollView {
                height: 150px;
                TextEdit { text: log-text; enabled: false; }
            }
            
            // Hidden timer for processing events
        }
        
        callback browse-dir();
        callback browse-out-dir();
        callback generate-code();
        callback start-transfer();
        callback cancel-transfer();
    }
}

// Event for communication between threads
#[derive(Debug, Clone)]
enum UiEvent {
    Log(String),
    BrowseDir(String),
    BrowseOutDir(String),
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
        let _ = self.tx.send(UiEvent::Log(format!("Analyse: {}/{}: {}", done, total, current)));
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

    // Create channel for events
    let (tx, rx) = channel();

    // Store receiver in Rc<RefCell> so we can access it from callbacks
    let rx_ref = Rc::new(RefCell::new(rx));

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
        let tx = tx_clone.clone();
        let code = crypto::gen_code();
        let _ = tx.send(UiEvent::SetCode(code));
    });

    let tx_clone = tx.clone();
    let ui_handle = ui.as_weak();
    ui.on_start_transfer(move || {
        let ui = ui_handle.unwrap();
        let tx = tx_clone.clone();
        start_transfer(&ui, tx);
    });

    let tx_clone = tx.clone();
    ui.on_cancel_transfer(move || {
        let _ = tx_clone.send(UiEvent::Cancel);
    });

    // Set up timer to process events
    let ui_handle = ui.as_weak();
    let rx_ref_timer = rx_ref.clone();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(50), move || {
        let rx = rx_ref_timer.borrow_mut();
        while let Ok(event) = rx.try_recv() {
            if let Some(ui) = ui_handle.upgrade() {
                process_event(&ui, event);
            }
        }
    });

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
            ui.set_dir(path.into());
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
        UiEvent::Cancel => {
            // Handled elsewhere
        }
    }
}

fn start_transfer(ui: &AppWindow, tx: Sender<UiEvent>) {
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

    // Set running state via event
    let _ = tx.send(UiEvent::SetRunning(true));
    let _ = tx.send(UiEvent::Log("Demarrage du transfert...".into()));

    let tx_thread = tx.clone();

    thread::spawn(move || {
        let rep = SlintReporter {
            tx: tx_thread.clone(),
        };
        let cancel = Arc::new(AtomicBool::new(false));

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

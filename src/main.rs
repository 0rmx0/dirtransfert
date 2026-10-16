fn main() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("panique : {}", info);
        let log_path = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("DirTransfert-erreur.log")))
            .unwrap_or_else(|| std::path::PathBuf::from("DirTransfert-erreur.log"));
        let _ = std::fs::write(
            &log_path,
            format!(
                "DirTransfert v{} - panique\n{}\n",
                env!("CARGO_PKG_VERSION"),
                msg
            ),
        );
        eprintln!("[panique] {}", msg);
    }));
    let argv: Vec<String> = std::env::args().collect();
    if std::env::var_os("WGPU_BACKEND").is_none() {
        std::env::set_var("WGPU_BACKEND", "dx12");
    }
    if argv.iter().any(|a| a == "--cli" || a == "--console") {
        let code = dirtransfer::cli::run(&argv);
        std::process::exit(code);
    }
    if let Err(e) = dirtransfer::gui::launch() {
        let msg = format!(
            "{}\n\nAstuce : lancez avec --cli pour l'interface en ligne de commande.",
            e
        );
        let log_path = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("DirTransfert-erreur.log")))
            .or_else(|| Some(std::path::PathBuf::from("DirTransfert-erreur.log")));
        if let Some(path) = log_path {
            let _ = std::fs::write(
                &path,
                format!(
                    "DirTransfert v{} - echec du lancement de la GUI\n{}\n",
                    env!("CARGO_PKG_VERSION"),
                    msg
                ),
            );
        }
        rfd::MessageDialog::new()
            .set_title("DirTransfert - erreur")
            .set_level(rfd::MessageLevel::Error)
            .set_description(msg)
            .show();
        eprintln!("[erreur] {}", e);
        eprintln!("Astuce : lancez avec --cli pour l'interface en ligne de commande.");
        std::process::exit(1);
    }
}

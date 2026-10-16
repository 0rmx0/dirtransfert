fn main() {
    let argv: Vec<String> = std::env::args().collect();
    if argv.iter().any(|a| a == "--cli" || a == "--console") {
        let code = dirtransfer::cli::run(&argv);
        std::process::exit(code);
    }
    if let Err(e) = dirtransfer::gui::launch() {
        let msg = format!(
            "{}\n\nAstuce : lancez avec --cli pour l'interface en ligne de commande.",
            e
        );
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

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    if argv.iter().any(|a| a == "--cli" || a == "--console") {
        let code = dirtransfer::cli::run(&argv);
        std::process::exit(code);
    }
    if let Err(e) = dirtransfer::gui::launch() {
        eprintln!("[erreur] {}", e);
        eprintln!("Astuce : lancez avec --cli pour l'interface en ligne de commande.");
        std::process::exit(1);
    }
}

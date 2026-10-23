fn main() {
    let argv: Vec<String> = std::env::args().collect();
    if argv.iter().any(|a| a == "--log" || a == "--verbose") {
        std::env::set_var("DIRTRANSFER_LOG", "1");
    }
    if argv.iter().any(|a| a == "--cli" || a == "--console") {
        let code = dirtransfer::cli::run(&argv);
        std::process::exit(code);
    }
    #[cfg(feature = "slint-ui")]
    if let Err(e) = dirtransfer::slint_main::run() {
        eprintln!("[erreur] {}", e);
        eprintln!("Astuce : lancez avec --cli pour l'interface en ligne de commande.");
        std::process::exit(1);
    }
    #[cfg(not(feature = "slint-ui"))]
    {
        eprintln!(
            "[erreur] interface graphique non compilee (recompilez avec la feature slint-ui)"
        );
        eprintln!("Astuce : lancez avec --cli pour l'interface en ligne de commande.");
        std::process::exit(1);
    }
}

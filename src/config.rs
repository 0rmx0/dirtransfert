//! Fichier de configuration optionnel : ~/.config/dirtransfert/config
//! (ou $DIRTRANSFERT_CONFIG). Format simple `cle = valeur`, `#` pour commenter.
//! Cles : pipeline, exclude (separees par des virgules), limit (Mo/s), auto-yes.

use std::path::PathBuf;

#[derive(Default, Clone, Debug)]
pub struct Config {
    pub pipeline: Option<usize>,
    pub exclude: Vec<String>,
    pub limit_mbps: Option<f64>,
    pub auto_yes: bool,
}

pub fn config_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("DIRTRANSFERT_CONFIG") {
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("dirtransfert")
            .join("config"),
    )
}

pub fn load() -> Config {
    let mut cfg = Config::default();
    let Some(path) = config_path() else {
        return cfg;
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return cfg;
    };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim();
        match k {
            "pipeline" => cfg.pipeline = v.parse::<usize>().ok().map(|n| n.clamp(1, 64)),
            "exclude" => cfg.exclude.extend(
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
            ),
            "limit" => cfg.limit_mbps = v.parse::<f64>().ok().filter(|x| *x > 0.0),
            "auto-yes" => cfg.auto_yes = matches!(v, "1" | "true" | "yes" | "oui"),
            _ => {}
        }
    }
    cfg
}

/// Ajoute une ligne a l'historique local des transferts.
pub fn append_history(line: &str) {
    let Ok(home) = std::env::var("HOME") else {
        return;
    };
    let dir = PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("dirtransfert");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let entry = format!("{} {}\n", ts, line);
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("history.log"))
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(entry.as_bytes())
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_config_simple() {
        let dir = std::env::temp_dir().join("dtx_config_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("config");
        std::fs::write(
            &f,
            "# commentaire\npipeline = 16\nexclude = .git, node_modules, *.tmp\nlimit = 50\nauto-yes = oui\ninconnu = x\n",
        )
        .unwrap();
        std::env::set_var("DIRTRANSFERT_CONFIG", &f);
        let cfg = load();
        std::env::remove_var("DIRTRANSFERT_CONFIG");
        assert_eq!(cfg.pipeline, Some(16));
        assert_eq!(cfg.exclude, vec![".git", "node_modules", "*.tmp"]);
        assert_eq!(cfg.limit_mbps, Some(50.0));
        assert!(cfg.auto_yes);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

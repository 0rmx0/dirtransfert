#[allow(unused_imports)]
use std::process::Command;

pub const RULE_NAME: &str = "DirTransfert";

#[cfg(windows)]
fn is_admin() -> bool {
    Command::new("net")
        .args(["session"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn netsh(args: &[&str]) -> Result<String, String> {
    let out = Command::new("netsh")
        .args(args)
        .output()
        .map_err(|e| format!("lancement de netsh : {}", e))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let msg = if msg.is_empty() {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        } else {
            msg
        };
        return Err(msg);
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(windows)]
pub fn ensure_firewall_rule(port: u16) -> Result<bool, String> {
    if !is_admin() {
        return Err("droits administrateur requis (lancez en tant qu'administrateur)".into());
    }
    let exists = Command::new("netsh")
        .args([
            "advfirewall",
            "firewall",
            "rule",
            "show",
            "rule",
            &format!("name={}", RULE_NAME),
        ])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(RULE_NAME))
        .unwrap_or(false);
    if exists {
        let _ = netsh(&[
            "advfirewall",
            "firewall",
            "rule",
            "delete",
            &format!("name={}", RULE_NAME),
        ]);
    }
    netsh(&[
        "advfirewall",
        "firewall",
        "rule",
        "add",
        &format!("name={}", RULE_NAME),
        "dir=in",
        "action=allow",
        "protocol=TCP",
        &format!("localport={}", port),
    ])
    .map(|_| true)
}

#[cfg(windows)]
pub fn remove_firewall_rule() -> Result<(), String> {
    if !is_admin() {
        return Err("droits administrateur requis".into());
    }
    netsh(&[
        "advfirewall",
        "firewall",
        "rule",
        "delete",
        &format!("name={}", RULE_NAME),
    ])
    .map(|_| ())
    .map_err(|e| {
        if e.contains("aucune") || e.to_lowercase().contains("no rules") {
            String::new()
        } else {
            e
        }
    })
}

#[cfg(not(windows))]
pub fn ensure_firewall_rule(_port: u16) -> Result<bool, String> {
    Err("gestion du pare-feu disponible uniquement sous Windows".into())
}

#[cfg(not(windows))]
pub fn remove_firewall_rule() -> Result<(), String> {
    Err("gestion du pare-feu disponible uniquement sous Windows".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn rule_name_stable() {
        assert_eq!(super::RULE_NAME, "DirTransfert");
    }
}

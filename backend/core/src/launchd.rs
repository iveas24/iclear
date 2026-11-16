//! Launch agents / daemons: what starts in the background and lets the user
//! disable agents in their own GUI domain.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use iclear_protocol::LaunchAgent;

fn uid() -> u32 {
    unsafe { libc::getuid() }
}

fn launchctl(args: &[&str]) -> Result<String, String> {
    let out = Command::new("/bin/launchctl").args(args).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn loaded_labels() -> HashSet<String> {
    launchctl(&["list"])
        .unwrap_or_default()
        .lines()
        .skip(1)
        .filter_map(|l| l.split('\t').nth(2).map(str::to_string))
        .collect()
}

fn disabled_labels() -> HashSet<String> {
    launchctl(&["print-disabled", &format!("gui/{}", uid())])
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains("=> disabled") || l.contains("=> true"))
        .filter_map(|l| l.split('"').nth(1).map(str::to_string))
        .collect()
}

fn read_dir(dir: &Path, scope: &str, loaded: &HashSet<String>, disabled: &HashSet<String>) -> Vec<LaunchAgent> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    rd.flatten()
        .filter(|e| e.path().extension().map(|x| x == "plist").unwrap_or(false))
        .filter_map(|e| {
            let path = e.path();
            let value: plist::Value = plist::from_file(&path).ok()?;
            let d = value.as_dictionary()?;
            let label = d.get("Label")?.as_string()?.to_string();
            let program = d
                .get("Program")
                .and_then(|v| v.as_string())
                .or_else(|| d.get("ProgramArguments")?.as_array()?.first()?.as_string())
                .map(str::to_string);
            let plist_disabled = d.get("Disabled").and_then(|v| v.as_boolean()).unwrap_or(false);
            Some(LaunchAgent {
                loaded: loaded.contains(&label),
                disabled: plist_disabled || disabled.contains(&label),
                label,
                path: path.to_string_lossy().to_string(),
                scope: scope.to_string(),
                program,
            })
        })
        .collect()
}

pub fn list() -> Vec<LaunchAgent> {
    let loaded = loaded_labels();
    let disabled = disabled_labels();
    let home = crate::storage::home();
    let mut all = Vec::new();
    all.extend(read_dir(&home.join("Library/LaunchAgents"), "user", &loaded, &disabled));
    all.extend(read_dir(Path::new("/Library/LaunchAgents"), "global-agent", &loaded, &disabled));
    all.extend(read_dir(Path::new("/Library/LaunchDaemons"), "global-daemon", &loaded, &disabled));
    all.sort_by(|a, b| a.scope.cmp(&b.scope).then(a.label.cmp(&b.label)));
    all
}

/// Enables/disables an agent in the user's GUI domain (persists across reboots).
/// Daemons run as root and are read-only here.
pub fn set_enabled(path: &str, enabled: bool) -> Result<(), String> {
    let agent = list().into_iter().find(|a| a.path == path).ok_or("No se encontró ese agente")?;
    if agent.scope == "global-daemon" {
        return Err("Los daemons del sistema no se pueden cambiar desde aquí".into());
    }
    let domain = format!("gui/{}", uid());
    let target = format!("{domain}/{}", agent.label);
    if enabled {
        launchctl(&["enable", &target])?;
        let _ = launchctl(&["bootstrap", &domain, path]);
    } else {
        launchctl(&["disable", &target])?;
        let _ = launchctl(&["bootout", &target]);
    }
    Ok(())
}

pub fn user_agents_dir() -> PathBuf {
    crate::storage::home().join("Library/LaunchAgents")
}

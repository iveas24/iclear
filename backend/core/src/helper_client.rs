//! Client for the privileged helper (`iclear-helper`) and its installer.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use iclear_protocol::{HelperRequest, HelperResponse, HelperStatus, HELPER_LABEL, HELPER_SOCKET};

pub const DAEMON_PLIST: &str = "/Library/LaunchDaemons/com.iclear.helper.plist";
pub const HELPER_BINARY: &str = "/Library/PrivilegedHelperTools/com.iclear.helper";

const INSTALL_SCRIPT: &str = include_str!("../../helper/scripts/install.sh");
const UNINSTALL_SCRIPT: &str = include_str!("../../helper/scripts/uninstall.sh");

pub fn send(req: &HelperRequest) -> Result<HelperResponse, String> {
    let mut stream =
        UnixStream::connect(HELPER_SOCKET).map_err(|e| format!("El helper no responde ({e}). ¿Está instalado?"))?;
    stream.set_read_timeout(Some(Duration::from_secs(120))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
    let mut line = serde_json::to_string(req).map_err(|e| e.to_string())?;
    line.push('\n');
    stream.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream);
    let mut resp = String::new();
    reader.read_line(&mut resp).map_err(|e| e.to_string())?;
    serde_json::from_str(&resp).map_err(|e| format!("respuesta inválida del helper: {e}"))
}

/// Sends a request and turns `ok: false` into an error.
pub fn call(req: &HelperRequest) -> Result<HelperResponse, String> {
    let resp = send(req)?;
    if resp.ok {
        Ok(resp)
    } else {
        Err(resp.message)
    }
}

pub fn status() -> HelperStatus {
    let installed = Path::new(DAEMON_PLIST).exists();
    match send(&HelperRequest::Status) {
        Ok(r) => HelperStatus { installed, reachable: true, state: r.state, error: None },
        Err(e) => HelperStatus { installed, reachable: false, state: None, error: Some(e) },
    }
}

/// Where the helper binary built next to the app lives (dev: target/debug, bundle: Contents/MacOS).
pub fn bundled_helper_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("ICLEAR_HELPER_PATH") {
        return Some(PathBuf::from(p));
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    ["iclear-helper", "../Resources/iclear-helper"].iter().map(|n| dir.join(n)).find(|p| p.exists())
}

fn run_as_admin(script: &str, args: &[&str]) -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("iclear-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let script_path = dir.join("script.sh");
    std::fs::write(&script_path, script).map_err(|e| e.to_string())?;
    let quoted: Vec<String> = std::iter::once(script_path.to_string_lossy().to_string())
        .chain(args.iter().map(|a| a.to_string()))
        .map(|a| format!("'{}'", a.replace('\'', "'\\''")))
        .collect();
    let shell = format!("/bin/sh {}", quoted.join(" "));
    let apple = format!(
        "do shell script \"{}\" with administrator privileges with prompt \"iclear necesita instalar su helper para controlar ventiladores y batería.\"",
        shell.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let out = Command::new("/usr/bin/osascript").args(["-e", &apple]).output().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&dir);
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn install(helper_binary: &Path) -> Result<(), String> {
    if !helper_binary.exists() {
        return Err(format!("No se encontró el binario del helper en {}", helper_binary.display()));
    }
    // Stage the binary outside TCC-protected folders (~/Desktop, ~/Documents…):
    // the root shell started by osascript is not allowed to read them.
    let staging = std::env::temp_dir().join(format!("iclear-helper-stage-{}", std::process::id()));
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let _ = std::fs::set_permissions(&staging, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    let staged = staging.join("iclear-helper");
    std::fs::copy(helper_binary, &staged).map_err(|e| format!("No se pudo preparar el helper: {e}"))?;
    let uid = unsafe { libc::getuid() }.to_string();
    let result = run_as_admin(INSTALL_SCRIPT, &[&staged.to_string_lossy(), &uid, HELPER_LABEL]);
    let _ = std::fs::remove_dir_all(&staging);
    result
}

pub fn uninstall() -> Result<(), String> {
    // Restore defaults before removing the daemon.
    let _ = send(&HelperRequest::SetChargeLimit { percent: None });
    let _ = send(&HelperRequest::SetForceDischarge { enabled: false });
    run_as_admin(UNINSTALL_SCRIPT, &[HELPER_LABEL])
}

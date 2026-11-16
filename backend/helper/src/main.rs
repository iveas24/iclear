//! iclear-helper: privileged launchd daemon.
//!
//! Listens on a Unix socket, accepts only the uid recorded at install time
//! (plus root), and executes a closed set of validated commands. It never
//! runs arbitrary shell input. See docs/HELPER_SECURITY.md.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use iclear_core::smc::Smc;
use iclear_core::{battery, sensors};
use iclear_protocol::{
    HelperRequest, HelperResponse, HelperState, OptimizeAction, HELPER_PROTOCOL_VERSION, HELPER_SOCKET,
};
use serde::{Deserialize, Serialize};

const CONF_DIR: &str = "/Library/Application Support/iclear";
const HYSTERESIS: u8 = 3;
const FAN_SAFETY_MAX_C: f32 = 95.0;

#[derive(Deserialize)]
struct Auth {
    allowed_uid: u32,
}

#[derive(Default, Serialize, Deserialize)]
struct Persisted {
    charge_limit: Option<u8>,
    force_discharge: bool,
}

/// How this firmware lets us stop charging.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ChargeBackend {
    /// macOS 15+/26 (M3/M4): CHTE ui32, 1 = inhibit.
    Chte,
    /// Older firmware (M1/M2): CH0B + CH0C, 0x02 = inhibit.
    Ch0bc,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum DischargeBackend {
    /// CHIE: 0x08 disconnects the adapter.
    Chie,
    /// CH0J (older): 0x01.
    Ch0j,
    None,
}

struct Helper {
    smc: Smc,
    charge: ChargeBackend,
    discharge: DischargeBackend,
    fan_control: bool,
    state: Mutex<HelperState>,
}

fn log(msg: &str) {
    eprintln!("[iclear-helper] {msg}");
}

/// A key is usable only if we can write its current value back (macOS 27 gates
/// some keys behind a private entitlement: they read fine but refuse writes).
fn writable(smc: &Smc, key: &str) -> bool {
    match smc.read(key) {
        Ok(v) => smc.write_bytes(key, &v.bytes).is_ok(),
        Err(_) => false,
    }
}

impl Helper {
    fn new() -> Result<Self, String> {
        let smc = Smc::open().map_err(|e| e.to_string())?;
        let charge = if writable(&smc, "CHTE") {
            ChargeBackend::Chte
        } else if writable(&smc, "CH0B") && writable(&smc, "CH0C") {
            ChargeBackend::Ch0bc
        } else {
            ChargeBackend::None
        };
        let discharge = if writable(&smc, "CHIE") {
            DischargeBackend::Chie
        } else if writable(&smc, "CH0J") {
            DischargeBackend::Ch0j
        } else {
            DischargeBackend::None
        };
        let fan_control = smc.read_f64("FNum").unwrap_or(0.0) > 0.0 && writable(&smc, "F0Md");
        log(&format!("backends: charge={charge:?} discharge={discharge:?} fans={fan_control}"));
        let state = HelperState {
            protocol_version: HELPER_PROTOCOL_VERSION,
            helper_version: env!("CARGO_PKG_VERSION").to_string(),
            supports_charge_limit: charge != ChargeBackend::None,
            supports_force_discharge: discharge != DischargeBackend::None,
            supports_fan_control: fan_control,
            ..Default::default()
        };
        Ok(Self { smc, charge, discharge, fan_control, state: Mutex::new(state) })
    }

    fn snapshot(&self) -> HelperState {
        self.state.lock().unwrap().clone()
    }

    fn persist(&self) {
        let s = self.state.lock().unwrap();
        let p = Persisted { charge_limit: s.charge_limit, force_discharge: s.force_discharge };
        let _ = std::fs::create_dir_all(CONF_DIR);
        let _ = std::fs::write(Path::new(CONF_DIR).join("helper-state.json"), serde_json::to_vec(&p).unwrap());
    }

    fn load_persisted(&self) {
        let Ok(bytes) = std::fs::read(Path::new(CONF_DIR).join("helper-state.json")) else { return };
        let Ok(p) = serde_json::from_slice::<Persisted>(&bytes) else { return };
        let mut s = self.state.lock().unwrap();
        s.charge_limit = p.charge_limit.filter(|_| s.supports_charge_limit);
        s.force_discharge = p.force_discharge && s.supports_force_discharge;
    }

    // ---- charging -------------------------------------------------------

    fn set_charging_inhibited(&self, inhibit: bool) -> Result<(), String> {
        let r = match self.charge {
            ChargeBackend::Chte => self.smc.write_bytes("CHTE", &if inhibit { [1, 0, 0, 0] } else { [0, 0, 0, 0] }),
            ChargeBackend::Ch0bc => {
                let v = if inhibit { 0x02 } else { 0x00 };
                self.smc.write_bytes("CH0B", &[v]).and_then(|_| self.smc.write_bytes("CH0C", &[v]))
            }
            ChargeBackend::None => return Err("Este Mac no permite controlar la carga por SMC".into()),
        };
        r.map_err(|e| e.to_string())?;
        self.state.lock().unwrap().charging_inhibited = inhibit;
        Ok(())
    }

    fn set_adapter_disabled(&self, disabled: bool) -> Result<(), String> {
        let r = match self.discharge {
            DischargeBackend::Chie => self.smc.write_bytes("CHIE", &[if disabled { 0x08 } else { 0x00 }]),
            DischargeBackend::Ch0j => self.smc.write_bytes("CH0J", &[if disabled { 0x01 } else { 0x00 }]),
            DischargeBackend::None => return Err("Este Mac no permite forzar la descarga".into()),
        };
        r.map_err(|e| e.to_string())
    }

    /// Called periodically: applies the charge limit with hysteresis.
    fn charge_tick(&self) {
        let (limit, force, inhibited) = {
            let s = self.state.lock().unwrap();
            (s.charge_limit, s.force_discharge, s.charging_inhibited)
        };
        let Some(b) = battery::read() else { return };
        if force {
            // Stop forcing once we are at/below the limit (or 20 % without a limit).
            let floor = limit.unwrap_or(20);
            if b.percent <= floor {
                let _ = self.set_adapter_disabled(false);
                self.state.lock().unwrap().force_discharge = false;
                self.persist();
                log("descarga forzada terminada: se alcanzó el límite");
            }
        }
        match limit {
            Some(l) if b.percent >= l && !inhibited => {
                let _ = self.set_charging_inhibited(true);
            }
            Some(l) if b.percent + HYSTERESIS <= l && inhibited => {
                let _ = self.set_charging_inhibited(false);
            }
            None if inhibited => {
                let _ = self.set_charging_inhibited(false);
            }
            _ => {}
        }
    }

    // ---- fans -----------------------------------------------------------

    fn fan_count(&self) -> u8 {
        self.smc.read_f64("FNum").unwrap_or(0.0) as u8
    }

    fn set_fan_manual(&self, fan: u8) -> Result<(), String> {
        let md = format!("F{fan}Md");
        if self.smc.write_bytes(&md, &[1]).is_ok() {
            return Ok(());
        }
        // M1–M3 firmware needs the test-mode unlock first; it can take a few seconds.
        if self.smc.has_key("Ftst") {
            self.smc.write_bytes("Ftst", &[1]).map_err(|e| e.to_string())?;
            for _ in 0..20 {
                thread::sleep(Duration::from_millis(250));
                if self.smc.write_bytes(&md, &[1]).is_ok() {
                    return Ok(());
                }
            }
        }
        Err("El firmware no aceptó el modo manual del ventilador".into())
    }

    fn set_fan_rpm(&self, fan: u8, rpm: f32) -> Result<(), String> {
        if !self.fan_control {
            return Err("Control de ventiladores no disponible en este Mac".into());
        }
        if fan >= self.fan_count() {
            return Err(format!("El ventilador {fan} no existe"));
        }
        let min = self.smc.read_f64(&format!("F{fan}Mn")).unwrap_or(0.0) as f32;
        let max = self.smc.read_f64(&format!("F{fan}Mx")).unwrap_or(0.0) as f32;
        if !(rpm.is_finite() && max > 0.0) {
            return Err("RPM inválidas".into());
        }
        let rpm = rpm.clamp(min, max);
        self.set_fan_manual(fan)?;
        self.smc.write_number(&format!("F{fan}Tg"), rpm as f64).map_err(|e| e.to_string())?;
        let mut s = self.state.lock().unwrap();
        if !s.manual_fans.contains(&fan) {
            s.manual_fans.push(fan);
        }
        Ok(())
    }

    fn set_fan_auto(&self, fan: u8) -> Result<(), String> {
        if fan >= self.fan_count() {
            return Err(format!("El ventilador {fan} no existe"));
        }
        self.smc.write_bytes(&format!("F{fan}Md"), &[0]).map_err(|e| e.to_string())?;
        let remaining = {
            let mut s = self.state.lock().unwrap();
            s.manual_fans.retain(|f| *f != fan);
            s.manual_fans.len()
        };
        if remaining == 0 && self.smc.has_key("Ftst") {
            let _ = self.smc.write_bytes("Ftst", &[0]);
        }
        Ok(())
    }

    fn all_fans_auto(&self) {
        for f in 0..self.fan_count() {
            let _ = self.smc.write_bytes(&format!("F{f}Md"), &[0]);
        }
        if self.smc.has_key("Ftst") {
            let _ = self.smc.write_bytes("Ftst", &[0]);
        }
        self.state.lock().unwrap().manual_fans.clear();
    }

    // ---- optimizations --------------------------------------------------

    fn optimize(&self, action: OptimizeAction) -> Result<String, String> {
        fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
            let out = Command::new(cmd).args(args).output().map_err(|e| format!("{cmd}: {e}"))?;
            if out.status.success() {
                Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                Err(format!("{cmd}: {}", String::from_utf8_lossy(&out.stderr).trim()))
            }
        }
        match action {
            OptimizeAction::PurgeMemory => run("/usr/sbin/purge", &[]).map(|_| "Memoria inactiva liberada".into()),
            OptimizeAction::FlushDns => {
                run("/usr/bin/dscacheutil", &["-flushcache"])?;
                run("/usr/bin/killall", &["-HUP", "mDNSResponder"])?;
                Ok("Caché DNS vaciada".into())
            }
            OptimizeAction::ThinLocalSnapshots => {
                let out = run("/usr/bin/tmutil", &["thinlocalsnapshots", "/", "999999999999", "4"])?;
                Ok(if out.is_empty() { "Snapshots locales reducidos".into() } else { out })
            }
            OptimizeAction::ReindexSpotlight => {
                run("/usr/bin/mdutil", &["-E", "/"])?;
                Ok("Spotlight se está reindexando".into())
            }
            OptimizeAction::CleanSystemCaches => {
                let (freed, errors) = clean_system_caches();
                Ok(format!(
                    "Cachés del sistema: {} liberados{}",
                    iclear_protocol::format_bytes(freed),
                    if errors > 0 { format!(" ({errors} elementos en uso se omitieron)") } else { String::new() }
                ))
            }
        }
    }
}

/// Deletes the direct children of /Library/Caches (never follows symlinks).
fn clean_system_caches() -> (u64, usize) {
    let root = Path::new("/Library/Caches");
    let mut freed = 0;
    let mut errors = 0;
    let Ok(rd) = std::fs::read_dir(root) else { return (0, 1) };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.parent() != Some(root) {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
        let size = iclear_core::storage::scan::path_size(&p).0;
        let r = if meta.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
        match r {
            Ok(()) => freed += size,
            Err(_) => errors += 1,
        }
    }
    (freed, errors)
}

fn peer_uid(stream: &UnixStream) -> Option<u32> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    let r = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    (r == 0).then_some(uid)
}

fn handle(helper: &Helper, req: HelperRequest) -> HelperResponse {
    let result: Result<String, String> = match req {
        HelperRequest::Ping => Ok("pong".into()),
        HelperRequest::Status => Ok("ok".into()),
        HelperRequest::SetFanAuto { fan } => helper.set_fan_auto(fan).map(|_| format!("Ventilador {fan} en automático")),
        HelperRequest::SetFanRpm { fan, rpm } => {
            helper.set_fan_rpm(fan, rpm).map(|_| format!("Ventilador {fan} fijado en {rpm:.0} RPM"))
        }
        HelperRequest::SetChargeLimit { percent } => match percent {
            Some(p) if !(20..=100).contains(&p) => Err("El límite debe estar entre 20 % y 100 %".into()),
            _ if !helper.snapshot().supports_charge_limit => Err(
                "macOS no permite controlar la carga por SMC en este Mac. Usa Ajustes del Sistema › Batería › Límite de carga."
                    .into(),
            ),
            p => {
                let p = p.filter(|v| *v < 100);
                helper.state.lock().unwrap().charge_limit = p;
                helper.persist();
                helper.charge_tick();
                Ok(match p {
                    Some(v) => format!("Límite de carga: {v} %"),
                    None => "Límite de carga desactivado".into(),
                })
            }
        },
        HelperRequest::SetForceDischarge { enabled } => helper.set_adapter_disabled(enabled).map(|_| {
            helper.state.lock().unwrap().force_discharge = enabled;
            helper.persist();
            if enabled { "Descargando con el adaptador conectado".into() } else { "Adaptador reactivado".into() }
        }),
        HelperRequest::Optimize { action } => helper.optimize(action),
    };
    match result {
        Ok(msg) => HelperResponse::ok(msg, Some(helper.snapshot())),
        Err(e) => HelperResponse { ok: false, message: e, state: Some(helper.snapshot()) },
    }
}

fn serve(helper: Arc<Helper>, stream: UnixStream, allowed_uid: u32) {
    match peer_uid(&stream) {
        Some(uid) if uid == allowed_uid || uid == 0 => {}
        other => {
            log(&format!("conexión rechazada de uid {other:?}"));
            return;
        }
    }
    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
    let mut writer = match stream.try_clone() {
        Ok(w) => w,
        Err(_) => return,
    };
    let reader = BufReader::new(stream);
    // One request per line; cap line length to avoid unbounded buffers.
    for line in reader.lines().take(64) {
        let Ok(line) = line else { break };
        if line.len() > 4096 {
            break;
        }
        let resp = match serde_json::from_str::<HelperRequest>(&line) {
            Ok(req) => handle(&helper, req),
            Err(e) => HelperResponse::err(format!("solicitud inválida: {e}")),
        };
        let mut out = serde_json::to_string(&resp).unwrap();
        out.push('\n');
        if writer.write_all(out.as_bytes()).is_err() {
            break;
        }
    }
}

fn main() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("iclear-helper debe ejecutarse como root (launchd)");
        std::process::exit(1);
    }
    let auth: Auth = std::fs::read(Path::new(CONF_DIR).join("helper-auth.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| {
            log("falta helper-auth.json; solo root podrá conectarse");
            Auth { allowed_uid: 0 }
        });

    let helper = match Helper::new() {
        Ok(h) => Arc::new(h),
        Err(e) => {
            log(&format!("no se pudo abrir el SMC: {e}"));
            std::process::exit(1);
        }
    };
    // Safe defaults on every start: fans automatic, then re-apply saved battery settings.
    helper.all_fans_auto();
    helper.load_persisted();
    let force = helper.snapshot().force_discharge;
    let _ = helper.set_adapter_disabled(force);
    helper.charge_tick();

    // Battery limit loop.
    {
        let h = helper.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(20));
            h.charge_tick();
        });
    }
    // Fan safety loop: back to automatic if the CPU gets too hot in manual mode.
    {
        let h = helper.clone();
        thread::spawn(move || {
            let sensors = sensors::Sensors::new();
            loop {
                thread::sleep(Duration::from_secs(5));
                if h.snapshot().manual_fans.is_empty() {
                    continue;
                }
                let hot = sensors
                    .as_ref()
                    .and_then(|s| sensors::summarize(&s.temps()).cpu_max)
                    .map(|t| t >= FAN_SAFETY_MAX_C)
                    .unwrap_or(false);
                if hot {
                    log("temperatura alta: ventiladores a automático");
                    h.all_fans_auto();
                }
            }
        });
    }

    let _ = std::fs::remove_file(HELPER_SOCKET);
    let listener = match UnixListener::bind(HELPER_SOCKET) {
        Ok(l) => l,
        Err(e) => {
            log(&format!("no se pudo crear el socket: {e}"));
            std::process::exit(1);
        }
    };
    let _ = std::fs::set_permissions(HELPER_SOCKET, std::fs::Permissions::from_mode(0o600));
    let path = std::ffi::CString::new(HELPER_SOCKET).unwrap();
    unsafe {
        libc::chown(path.as_ptr(), auth.allowed_uid, 0);
    }
    log(&format!("escuchando en {HELPER_SOCKET} (uid permitido {})", auth.allowed_uid));

    for stream in listener.incoming().flatten() {
        let h = helper.clone();
        let uid = auth.allowed_uid;
        thread::spawn(move || serve(h, stream, uid));
    }
}

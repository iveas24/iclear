//! Memory intelligence.
//!
//! - `breakdown()`: Activity Monitor buckets (app, wired, compressed, cache)
//!   plus pressure and swap.
//! - `MemoryIntel`: groups every process under the app *responsible* for it
//!   (the same private API Activity Monitor uses, so Chrome renderers, WebKit
//!   content processes or a Docker VM are charged to their app), measures
//!   `phys_footprint`, remembers which app was in front and when, samples
//!   footprints to detect leaks, and turns all that into plain-language
//!   insights.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use iclear_protocol::{
    AppKind, AppMemory, Insight, InsightAction, MemoryBreakdown, MemoryPressure, MemoryReport, OptimizeAction, Severity,
};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

extern "C" {
    /// Private libSystem API: pid of the app responsible for `pid`
    /// (XPC services and helpers report their host app).
    fn responsibility_get_pid_responsible_for_pid(pid: libc::pid_t) -> libc::pid_t;
}

const SAMPLE_EVERY: Duration = Duration::from_secs(30);
const SAMPLE_WINDOW_SECS: u64 = 2 * 3600;
const FOCUS_SAVE_EVERY: Duration = Duration::from_secs(60);
/// Loose system processes smaller than this are folded into one row.
const FOLD_BELOW: u64 = 80 * 1024 * 1024;

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// 1024-based like Activity Monitor, with a decimal comma.
pub fn fmt_mem(bytes: u64) -> String {
    let b = bytes as f64;
    let s = if b >= GIB {
        format!("{:.1} GB", b / GIB)
    } else if b >= 1024.0 * 1024.0 {
        format!("{:.0} MB", b / 1024.0 / 1024.0)
    } else {
        format!("{:.0} KB", b / 1024.0)
    };
    s.replace('.', ",")
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// System-wide breakdown
// ---------------------------------------------------------------------------

fn sysctl_raw<T: Copy>(name: &str) -> Option<T> {
    let cname = std::ffi::CString::new(name).ok()?;
    let mut value: T = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<T>();
    let r = unsafe { libc::sysctlbyname(cname.as_ptr(), &mut value as *mut T as *mut _, &mut len, std::ptr::null_mut(), 0) };
    (r == 0).then_some(value)
}

fn sysctl_int(name: &str) -> Option<u64> {
    // Some of these are 32-bit, others 64-bit; read into 8 bytes and honor the size returned.
    let cname = std::ffi::CString::new(name).ok()?;
    let mut buf = [0u8; 8];
    let mut len = buf.len();
    let r = unsafe { libc::sysctlbyname(cname.as_ptr(), buf.as_mut_ptr() as *mut _, &mut len, std::ptr::null_mut(), 0) };
    if r != 0 {
        return None;
    }
    Some(match len {
        4 => u32::from_ne_bytes(buf[..4].try_into().ok()?) as u64,
        8 => u64::from_ne_bytes(buf),
        _ => return None,
    })
}

/// Parses `vm_stat` output into (page_size, name → pages).
fn parse_vm_stat(out: &str) -> (u64, HashMap<String, u64>) {
    let page = out
        .lines()
        .next()
        .and_then(|l| l.split("page size of ").nth(1))
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(16384);
    let map = out
        .lines()
        .skip(1)
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            let v = v.trim().trim_end_matches('.').parse().ok()?;
            Some((k.trim().trim_matches('"').to_string(), v))
        })
        .collect();
    (page, map)
}

pub fn breakdown() -> MemoryBreakdown {
    let out = Command::new("/usr/bin/vm_stat").output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    let (page, vm) = parse_vm_stat(&out);
    let p = |k: &str| vm.get(k).copied().unwrap_or(0) * page;

    let purgeable = p("Pages purgeable");
    let swap: Option<libc::xsw_usage> = sysctl_raw("vm.swapusage");
    let pressure = match sysctl_int("kern.memorystatus_vm_pressure_level").unwrap_or(1) {
        4 => MemoryPressure::Critical,
        2 => MemoryPressure::Warning,
        _ => MemoryPressure::Normal,
    };
    MemoryBreakdown {
        total: sysctl_int("hw.memsize").unwrap_or(0),
        app: p("Anonymous pages").saturating_sub(purgeable),
        wired: p("Pages wired down"),
        compressed: p("Pages occupied by compressor"),
        compressed_original: p("Pages stored in compressor"),
        cached: p("File-backed pages") + purgeable,
        free: p("Pages free") + p("Pages speculative"),
        swap_used: swap.map(|s| s.xsu_used).unwrap_or(0),
        swap_total: swap.map(|s| s.xsu_total).unwrap_or(0),
        pressure,
        free_percent: sysctl_int("kern.memorystatus_level").unwrap_or(0).min(100) as u8,
        swapouts_bytes: p("Swapouts"),
    }
}

/// 0-100. Driven by the kernel's own free level, penalized by swap and capped by pressure.
pub fn score(b: &MemoryBreakdown) -> u8 {
    let swap_gb = b.swap_used as f64 / GIB;
    let mut s = b.free_percent as f64 - (swap_gb * 3.0).min(30.0);
    s = match b.pressure {
        MemoryPressure::Normal => s.max(60.0),
        MemoryPressure::Warning => s.min(59.0),
        MemoryPressure::Critical => s.min(25.0),
    };
    s.clamp(0.0, 100.0).round() as u8
}

// ---------------------------------------------------------------------------
// Per-process data
// ---------------------------------------------------------------------------

/// `phys_footprint`: what Activity Monitor shows as "Memory" (includes compressed pages).
/// Only works for processes we may inspect (our own user).
pub fn footprint(pid: u32) -> Option<u64> {
    let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    let r = unsafe {
        libc::proc_pid_rusage(pid as libc::c_int, libc::RUSAGE_INFO_V4, &mut info as *mut _ as *mut libc::rusage_info_t)
    };
    (r == 0).then_some(info.ri_phys_footprint)
}

fn responsible(pid: u32) -> Option<u32> {
    let r = unsafe { responsibility_get_pid_responsible_for_pid(pid as libc::pid_t) };
    (r > 0).then_some(r as u32)
}

/// Outermost `X.app` in a path (helpers live in nested bundles).
pub fn outermost_app(path: &Path) -> Option<PathBuf> {
    let mut acc = PathBuf::new();
    for c in path.components() {
        acc.push(c);
        if acc.extension().map(|e| e == "app").unwrap_or(false) {
            return Some(acc);
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct BundleMeta {
    pub id: Option<String>,
    pub name: String,
    pub version: Option<String>,
    pub executable: Option<String>,
    /// LSUIElement / LSBackgroundOnly: menu bar or faceless app.
    pub background: bool,
    pub electron: bool,
    /// Lives in /System/Library (Finder, Dock...).
    pub system: bool,
}

pub fn read_bundle(path: &Path) -> Option<BundleMeta> {
    let info: plist::Dictionary = plist::from_file(path.join("Contents/Info.plist")).ok()?;
    let s = |k: &str| info.get(k).and_then(|v| v.as_string()).map(str::to_string);
    let b = |k: &str| {
        info.get(k).map(|v| v.as_boolean().unwrap_or(false) || v.as_string().map(|s| s == "1" || s == "YES").unwrap_or(false) || v.as_signed_integer() == Some(1)).unwrap_or(false)
    };
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    Some(BundleMeta {
        id: s("CFBundleIdentifier"),
        name: clean_name(&s("CFBundleDisplayName").or_else(|| s("CFBundleName")).filter(|n| !n.is_empty()).unwrap_or(stem)),
        version: s("CFBundleShortVersionString").or_else(|| s("CFBundleVersion")),
        executable: s("CFBundleExecutable"),
        background: b("LSUIElement") || b("LSBackgroundOnly"),
        electron: path.join("Contents/Frameworks/Electron Framework.framework").exists(),
        system: path.starts_with("/System/Library"),
    })
}

const BROWSERS: &[&str] = &[
    "com.google.chrome",
    "com.google.chrome.canary",
    "com.apple.safari",
    "com.apple.safaritechnologypreview",
    "com.microsoft.edgemac",
    "company.thebrowser.browser",
    "company.thebrowser.dia",
    "com.brave.browser",
    "org.mozilla.firefox",
    "com.operasoftware.opera",
    "com.vivaldi.vivaldi",
    "org.chromium.chromium",
    "app.zen-browser.zen",
    "ai.perplexity.comet",
    "com.openai.atlas",
];
const DEVELOPER: &[&str] = &[
    "com.apple.dt.xcode",
    "com.microsoft.vscode",
    "com.todesktop.230313mzl4w4u92", // Cursor
    "dev.zed.zed",
    "com.sublimetext.4",
    "com.google.android.studio",
    "com.apple.iphonesimulator",
];
const VIRTUALIZATION: &[&str] = &["com.docker.docker", "com.utmapp.utm", "dev.kdrag0n.macvirt", "com.parallels.", "com.vmware.fusion", "com.apple.virtualization"];

fn kind_for(meta: &BundleMeta) -> AppKind {
    let id = meta.id.as_deref().unwrap_or("").to_lowercase();
    if BROWSERS.contains(&id.as_str()) {
        AppKind::Browser
    } else if VIRTUALIZATION.iter().any(|v| id.starts_with(v)) {
        AppKind::Virtualization
    } else if DEVELOPER.contains(&id.as_str()) || id.starts_with("com.jetbrains.") {
        AppKind::Developer
    } else if meta.electron {
        AppKind::Electron
    } else if meta.system {
        AppKind::System
    } else if meta.background {
        AppKind::Background
    } else {
        AppKind::App
    }
}

fn loose_kind(name: &str, exe: Option<&Path>) -> AppKind {
    let lname = name.to_lowercase();
    if lname.contains("virtualization.virtualmachine") || lname.starts_with("qemu") || lname.contains("vmnetd") {
        return AppKind::Virtualization;
    }
    let Some(exe) = exe else { return AppKind::System };
    let s = exe.to_string_lossy();
    let home = std::env::var("HOME").unwrap_or_default();
    if s.starts_with("/System/") || s.starts_with("/usr/") || s.starts_with("/sbin/") || s.starts_with("/bin/") || s.starts_with("/Library/Apple/") {
        AppKind::System
    } else if s.starts_with("/opt/homebrew") || s.starts_with("/usr/local") || (!home.is_empty() && (s.starts_with(&format!("{home}/.")) || s.starts_with(&format!("{home}/Library/pnpm"))))
        || ["node", "python", "python3", "java", "ruby", "deno", "bun", "postgres", "redis-server", "mysqld", "cargo", "rustc", "rust-analyzer"].contains(&lname.as_str())
    {
        AppKind::Developer
    } else {
        AppKind::Background
    }
}

/// The process that actually holds a VM's RAM (Docker, Podman, UTM, Lima...).
fn is_vm_process(name: &str) -> bool {
    name.contains("Virtualization.VirtualMachine") || name.starts_with("qemu-system") || name == "vfkit" || name == "krunkit"
}

/// Strips invisible bidi marks some apps put in their names.
fn clean_name(name: &str) -> String {
    name.chars().filter(|c| !matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')).collect::<String>().trim().to_string()
}

fn is_renderer(name: &str) -> bool {
    name.contains("Renderer") || name.contains("WebContent")
}

/// Bundle id of the app in front, via LaunchServices.
fn front_bundle_id() -> Option<String> {
    let asn = Command::new("/usr/bin/lsappinfo").arg("front").output().ok()?;
    let asn = String::from_utf8_lossy(&asn.stdout).trim().to_string();
    if asn.is_empty() {
        return None;
    }
    let info = Command::new("/usr/bin/lsappinfo").args(["info", "-only", "bundleid", &asn]).output().ok()?;
    let text = String::from_utf8_lossy(&info.stdout);
    // Formats seen: `"LSBundleID"="com.foo"` or `bundleID="com.foo"`.
    text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        k.to_lowercase().contains("bundleid").then(|| v.trim().trim_matches('"').to_string()).filter(|v| !v.is_empty() && v != "[ NULL ]")
    })
}

fn focus_file() -> PathBuf {
    crate::storage::categories::home().join("Library/Application Support/iclear/focus.json")
}

// ---------------------------------------------------------------------------
// Stateful tracker
// ---------------------------------------------------------------------------

pub struct MemoryIntel {
    sys: System,
    bundles: HashMap<PathBuf, Option<BundleMeta>>,
    /// bundle id → unix secs when it was last seen in front.
    focus: HashMap<String, u64>,
    last_focus_save: Instant,
    focus_dirty: bool,
    /// app id → (unix secs, footprint)
    samples: HashMap<String, VecDeque<(u64, u64)>>,
    last_sample: Option<Instant>,
    started: Instant,
}

impl Default for MemoryIntel {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryIntel {
    pub fn new() -> Self {
        let focus = std::fs::read(focus_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self {
            sys: System::new(),
            bundles: HashMap::new(),
            focus,
            last_focus_save: Instant::now(),
            focus_dirty: false,
            samples: HashMap::new(),
            last_sample: None,
            started: Instant::now(),
        }
    }

    /// Last time each bundle id was seen in front (unix secs). Shared with the apps scanner.
    pub fn focus_map(&self) -> &HashMap<String, u64> {
        &self.focus
    }

    /// Call every few seconds: records the app in front and, every 30 s,
    /// samples per-app footprints for leak detection.
    pub fn tick(&mut self) {
        if let Some(id) = front_bundle_id() {
            self.focus.insert(id, now_secs());
            self.focus_dirty = true;
        }
        if self.focus_dirty && self.last_focus_save.elapsed() >= FOCUS_SAVE_EVERY {
            self.save_focus();
        }
        if self.last_sample.map(|t| t.elapsed() >= SAMPLE_EVERY).unwrap_or(true) {
            let apps = self.collect_apps();
            self.record(&apps);
        }
    }

    pub fn save_focus(&mut self) {
        let path = focus_file();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec(&self.focus) {
            let _ = std::fs::write(path, json);
        }
        self.focus_dirty = false;
        self.last_focus_save = Instant::now();
    }

    fn record(&mut self, apps: &[AppMemory]) {
        let t = now_secs();
        for a in apps {
            let q = self.samples.entry(a.id.clone()).or_default();
            q.push_back((t, a.footprint_bytes));
            while q.front().map(|(ts, _)| t - ts > SAMPLE_WINDOW_SECS).unwrap_or(false) {
                q.pop_front();
            }
        }
        self.samples.retain(|id, _| apps.iter().any(|a| &a.id == id));
        self.last_sample = Some(Instant::now());
    }

    fn bundle(&mut self, path: &Path) -> Option<BundleMeta> {
        self.bundles.entry(path.to_path_buf()).or_insert_with(|| read_bundle(path)).clone()
    }

    fn collect_apps(&mut self) -> Vec<AppMemory> {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory().with_user(UpdateKind::OnlyIfNotSet).with_exe(UpdateKind::OnlyIfNotSet),
        );
        let me = unsafe { libc::getuid() };
        let now = now_secs();

        struct Proc {
            pid: u32,
            name: String,
            exe: Option<PathBuf>,
            footprint: u64,
            cpu: f32,
            run_time: u64,
            mine: bool,
        }
        let procs: HashMap<u32, Proc> = self
            .sys
            .processes()
            .iter()
            .filter(|(pid, _)| pid.as_u32() != 0)
            .map(|(pid, p)| {
                let pid = pid.as_u32();
                (
                    pid,
                    Proc {
                        pid,
                        name: p.name().to_string_lossy().to_string(),
                        exe: p.exe().map(Path::to_path_buf),
                        footprint: footprint(pid).unwrap_or_else(|| p.memory()),
                        cpu: p.cpu_usage(),
                        run_time: p.run_time(),
                        mine: p.user_id().map(|u| **u) == Some(me),
                    },
                )
            })
            .collect();

        let mut groups: HashMap<String, AppMemory> = HashMap::new();
        for p in procs.values() {
            let owner = responsible(p.pid).and_then(|r| procs.get(&r)).unwrap_or(p);
            let bundle_path = owner.exe.as_deref().and_then(outermost_app).or_else(|| p.exe.as_deref().and_then(outermost_app));
            let meta = bundle_path.as_deref().and_then(|bp| self.bundle(bp));

            let (id, name, bundle_id, kind, is_gui) = match (&bundle_path, &meta) {
                (Some(bp), Some(m)) => {
                    let kind = kind_for(m);
                    let gui = !m.background && !m.system;
                    (bp.to_string_lossy().to_string(), m.name.clone(), m.id.clone(), kind, gui)
                }
                _ => {
                    let kind = loose_kind(&owner.name, owner.exe.as_deref());
                    (format!("proc:{}", owner.name), owner.name.clone(), None, kind, false)
                }
            };

            let g = groups.entry(id.clone()).or_insert_with(|| AppMemory {
                id,
                name,
                bundle_id,
                kind,
                footprint_bytes: 0,
                process_count: 0,
                renderer_count: 0,
                cpu: 0.0,
                pids: vec![],
                main_pid: None,
                killable: true,
                is_gui,
                running_secs: 0,
                last_focused_secs_ago: None,
                growth_per_hour: None,
            });
            g.footprint_bytes += p.footprint;
            g.process_count += 1;
            g.cpu += p.cpu;
            g.pids.push(p.pid);
            g.killable &= p.mine;
            if is_vm_process(&p.name) {
                g.kind = AppKind::Virtualization;
            }
            if is_renderer(&p.name) {
                g.renderer_count += 1;
            }
            // Main pid: the responsible process, preferring the bundle's own executable.
            let is_main_exe = match (&bundle_path, owner.exe.as_deref()) {
                (Some(bp), Some(exe)) => exe.parent() == Some(bp.join("Contents/MacOS").as_path()),
                _ => false,
            };
            if g.main_pid.is_none() || is_main_exe {
                g.main_pid = Some(owner.pid);
                g.running_secs = owner.run_time;
            }
        }

        // Fold small loose system/background processes.
        let mut apps: Vec<AppMemory> = Vec::new();
        let mut folded: HashMap<AppKind, AppMemory> = HashMap::new();
        for (_, mut a) in groups {
            if a.id.starts_with("proc:") && a.footprint_bytes < FOLD_BELOW && matches!(a.kind, AppKind::System | AppKind::Background) {
                let f = folded.entry(a.kind).or_insert_with(|| AppMemory {
                    id: format!("folded:{:?}", a.kind),
                    name: if a.kind == AppKind::System { "Servicios de macOS".into() } else { "Otros procesos en segundo plano".into() },
                    bundle_id: None,
                    kind: a.kind,
                    footprint_bytes: 0,
                    process_count: 0,
                    renderer_count: 0,
                    cpu: 0.0,
                    pids: vec![],
                    main_pid: None,
                    killable: false,
                    is_gui: false,
                    running_secs: 0,
                    last_focused_secs_ago: None,
                    growth_per_hour: None,
                });
                f.footprint_bytes += a.footprint_bytes;
                f.process_count += a.process_count;
                f.cpu += a.cpu;
                f.pids.append(&mut a.pids);
                continue;
            }
            if let Some(bid) = &a.bundle_id {
                a.last_focused_secs_ago = self.focus.get(bid).map(|t| now.saturating_sub(*t));
            }
            apps.push(a);
        }
        apps.extend(folded.into_values());
        apps.sort_by(|a, b| b.footprint_bytes.cmp(&a.footprint_bytes));
        apps
    }

    fn growth(&self, id: &str) -> Option<i64> {
        let q = self.samples.get(id)?;
        let (t0, f0) = *q.front()?;
        let (t1, f1) = *q.back()?;
        let dt = t1.saturating_sub(t0);
        if dt < 10 * 60 {
            return None;
        }
        Some(((f1 as i64 - f0 as i64) as f64 * 3600.0 / dt as f64) as i64)
    }

    pub fn report(&mut self) -> MemoryReport {
        let mut apps = self.collect_apps();
        if self.last_sample.map(|t| t.elapsed() >= SAMPLE_EVERY).unwrap_or(true) {
            self.record(&apps);
        }
        for a in &mut apps {
            a.growth_per_hour = self.growth(&a.id);
        }
        let breakdown = breakdown();
        let observed_secs = self.started.elapsed().as_secs();
        let front = front_bundle_id();
        let insights = insights(&breakdown, &apps, observed_secs, System::uptime(), front.as_deref());
        MemoryReport { score: score(&breakdown), breakdown, apps, insights, observed_secs }
    }
}

// ---------------------------------------------------------------------------
// Insights
// ---------------------------------------------------------------------------

fn human_duration(secs: u64) -> String {
    match secs {
        s if s < 3600 => format!("{} min", (s / 60).max(1)),
        s if s < 86400 => {
            let h = s / 3600;
            format!("{h} {}", if h == 1 { "hora" } else { "horas" })
        }
        s => {
            let d = s / 86400;
            format!("{d} {}", if d == 1 { "día" } else { "días" })
        }
    }
}

fn names(apps: &[&AppMemory], max: usize) -> String {
    let mut v: Vec<&str> = apps.iter().take(max).map(|a| a.name.as_str()).collect();
    if apps.len() > max {
        v.push("otras");
    }
    match v.len() {
        0 => String::new(),
        1 => v[0].to_string(),
        n => format!("{} y {}", v[..n - 1].join(", "), v[n - 1]),
    }
}

fn browser_tip(bundle_id: Option<&str>) -> &'static str {
    match bundle_id.unwrap_or("").to_lowercase().as_str() {
        "com.google.chrome" => "Activa Ajustes › Rendimiento › Ahorro de memoria y cierra pestañas que no estés usando.",
        "com.microsoft.edgemac" => "Activa las pestañas en reposo (Ajustes › Sistema y rendimiento).",
        "company.thebrowser.browser" => "Configura el archivado automático de pestañas en Arc.",
        "org.mozilla.firefox" => "Cierra pestañas o revisa about:memory y about:processes.",
        "com.apple.safari" => "Cierra pestañas o ventanas que no uses; Safari suspende las de fondo, pero no las elimina.",
        _ => "Cierra pestañas que no estés usando y revisa extensiones que no necesites.",
    }
}

pub fn insights(b: &MemoryBreakdown, apps: &[AppMemory], observed_secs: u64, uptime_secs: u64, front: Option<&str>) -> Vec<Insight> {
    let mut out = Vec::new();
    let total = b.total.max(1);
    let demand = b.app + b.wired + b.compressed_original;

    // 1. Overall diagnosis.
    match b.pressure {
        MemoryPressure::Normal if b.swap_used < 1 << 30 => out.push(Insight {
            id: "pressure".into(),
            severity: Severity::Info,
            title: "La memoria está sana".into(),
            detail: format!(
                "macOS usa {} como caché de archivos para que todo abra más rápido; eso no es un problema y se libera al instante si una app lo necesita.",
                fmt_mem(b.cached)
            ),
            savings_bytes: None,
            action: None,
        }),
        p => {
            let sev = if p == MemoryPressure::Critical { Severity::Critical } else if p == MemoryPressure::Warning { Severity::Warning } else { Severity::Suggestion };
            let mut detail = format!(
                "Tus apps y el sistema piden unos {} y el Mac tiene {}. Para que alcance, macOS comprime {} en {}",
                fmt_mem(demand),
                fmt_mem(total),
                fmt_mem(b.compressed_original),
                fmt_mem(b.compressed)
            );
            if b.swap_used > 0 {
                detail.push_str(&format!(" y guarda {} en el SSD (swap)", fmt_mem(b.swap_used)));
            }
            detail.push_str(". Comprimir y leer del disco gasta CPU y hace que todo se sienta más lento.");
            out.push(Insight {
                id: "pressure".into(),
                severity: sev,
                title: match p {
                    MemoryPressure::Critical => "Memoria al límite".into(),
                    MemoryPressure::Warning => "Tu Mac está bajo presión de memoria".into(),
                    MemoryPressure::Normal => "Tu Mac está usando swap".into(),
                },
                detail,
                savings_bytes: None,
                action: None,
            });
        }
    }

    // 2. Heavy GUI apps that are open but not being used.
    let mut idle: Vec<(&AppMemory, String)> = apps
        .iter()
        .filter(|a| a.is_gui && a.killable && a.main_pid.is_some() && a.footprint_bytes >= 300 << 20)
        .filter(|a| a.bundle_id.as_deref() != front && a.bundle_id.as_deref() != Some("com.iclear.app"))
        .filter_map(|a| match a.last_focused_secs_ago {
            Some(s) if s >= 30 * 60 => Some((a, format!("No la usas hace {}", human_duration(s)))),
            // Never seen in front while we've been watching for a while.
            None if observed_secs >= 30 * 60 && a.running_secs >= 30 * 60 => {
                Some((a, format!("Lleva {} abierta y no la has usado desde que iclear está observando", human_duration(a.running_secs))))
            }
            None if a.cpu < 0.5 && a.running_secs >= 6 * 3600 => Some((a, format!("Lleva {} abierta sin actividad", human_duration(a.running_secs)))),
            _ => None,
        })
        .collect();
    idle.sort_by(|a, b| b.0.footprint_bytes.cmp(&a.0.footprint_bytes));
    for (a, why) in idle.into_iter().take(5) {
        out.push(Insight {
            id: format!("idle:{}", a.id),
            severity: if b.pressure == MemoryPressure::Normal { Severity::Suggestion } else { Severity::Warning },
            title: format!("{} está abierta sin uso y ocupa {}", a.name, fmt_mem(a.footprint_bytes)),
            detail: format!("{why}. Cerrarla libera esa memoria; puedes volver a abrirla cuando la necesites."),
            savings_bytes: Some(a.footprint_bytes),
            action: Some(InsightAction::QuitApp { app_id: a.id.clone(), main_pid: a.main_pid.unwrap_or(0) }),
        });
    }

    // 3. Browsers with many tabs/renderers.
    for a in apps.iter().filter(|a| a.kind == AppKind::Browser && (a.renderer_count >= 15 || a.footprint_bytes >= 2 << 30)) {
        out.push(Insight {
            id: format!("browser:{}", a.id),
            severity: if a.footprint_bytes as f64 > total as f64 * 0.2 { Severity::Warning } else { Severity::Suggestion },
            title: format!("{} usa {} en {} procesos", a.name, fmt_mem(a.footprint_bytes), a.process_count),
            detail: format!(
                "{} de ellos son pestañas, extensiones o frames: cada uno es un proceso aparte. {}",
                a.renderer_count,
                browser_tip(a.bundle_id.as_deref())
            ),
            savings_bytes: Some(a.footprint_bytes / 2),
            action: None,
        });
    }

    // 4. Several Electron apps, each shipping its own Chromium.
    let electron: Vec<&AppMemory> = apps.iter().filter(|a| a.kind == AppKind::Electron && a.footprint_bytes >= 150 << 20).collect();
    if electron.len() >= 2 {
        let sum: u64 = electron.iter().map(|a| a.footprint_bytes).sum();
        out.push(Insight {
            id: "electron".into(),
            severity: Severity::Suggestion,
            title: format!("{} apps hechas con Electron suman {}", electron.len(), fmt_mem(sum)),
            detail: format!(
                "{} incluyen cada una su propio navegador Chromium. Si alguna tiene versión web, usarla como pestaña en tu navegador gasta mucho menos.",
                names(&electron, 4)
            ),
            savings_bytes: None,
            action: None,
        });
    }

    // 5. Virtual machines reserve RAM up front.
    for a in apps.iter().filter(|a| a.kind == AppKind::Virtualization && a.footprint_bytes >= 1 << 30) {
        out.push(Insight {
            id: format!("vm:{}", a.id),
            severity: Severity::Warning,
            title: format!("{} reserva {} para su máquina virtual", a.name, fmt_mem(a.footprint_bytes)),
            detail: "Las máquinas virtuales (Docker, UTM, Parallels…) apartan la memoria aunque no la ocupen. Baja el límite en sus ajustes de recursos o ciérrala cuando no la uses.".into(),
            savings_bytes: Some(a.footprint_bytes),
            action: a.main_pid.filter(|_| a.killable).map(|pid| InsightAction::QuitApp { app_id: a.id.clone(), main_pid: pid }),
        });
    }

    // 6. Leaks: steady growth over the observed window.
    for a in apps.iter().filter(|a| a.footprint_bytes >= 400 << 20 && a.growth_per_hour.map(|g| g >= 300 << 20).unwrap_or(false)) {
        out.push(Insight {
            id: format!("leak:{}", a.id),
            severity: Severity::Warning,
            title: format!("{} no para de crecer (+{}/h)", a.name, fmt_mem(a.growth_per_hour.unwrap_or(0) as u64)),
            detail: format!(
                "Su memoria sube de forma sostenida mientras iclear la observa, típico de una fuga de memoria. Reiniciarla vuelve a dejarla en un tamaño normal (hoy usa {}).",
                fmt_mem(a.footprint_bytes)
            ),
            savings_bytes: Some(a.footprint_bytes / 2),
            action: a.main_pid.filter(|_| a.killable && a.is_gui).map(|pid| InsightAction::QuitApp { app_id: a.id.clone(), main_pid: pid }),
        });
    }

    // 7. Background apps (menu bar items, updaters, sync agents).
    let bg: Vec<&AppMemory> = apps.iter().filter(|a| a.kind == AppKind::Background && a.footprint_bytes >= 50 << 20).collect();
    let bg_sum: u64 = bg.iter().map(|a| a.footprint_bytes).sum();
    if bg_sum >= 600 << 20 {
        out.push(Insight {
            id: "background".into(),
            severity: Severity::Suggestion,
            title: format!("Apps en segundo plano usan {}", fmt_mem(bg_sum)),
            detail: format!(
                "{} corren sin ventana (barra de menú, actualizadores, sincronización). Revisa cuáles se abren al iniciar sesión y desactiva las que no necesites.",
                names(&bg, 4)
            ),
            savings_bytes: Some(bg_sum / 2),
            action: Some(InsightAction::OpenPage { page: "optimize".into() }),
        });
    }

    // 8. Spotlight indexing.
    let mds: u64 = apps.iter().filter(|a| a.name.starts_with("mds") || a.name.starts_with("mdworker")).map(|a| a.footprint_bytes).sum();
    if mds >= 500 << 20 {
        out.push(Insight {
            id: "spotlight".into(),
            severity: Severity::Info,
            title: format!("Spotlight está indexando ({})", fmt_mem(mds)),
            detail: "Suele pasar tras actualizar macOS o copiar muchos archivos, y se calma solo. Si dura días, reindexar desde Optimizar lo arregla.".into(),
            savings_bytes: None,
            action: Some(InsightAction::OpenPage { page: "optimize".into() }),
        });
    }

    // 9. WindowServer.
    if let Some(ws) = apps.iter().find(|a| a.name == "WindowServer" && a.footprint_bytes >= 1536 << 20) {
        out.push(Insight {
            id: "windowserver".into(),
            severity: Severity::Info,
            title: format!("El servidor de ventanas usa {}", fmt_mem(ws.footprint_bytes)),
            detail: "Crece con pantallas externas de alta resolución, muchas ventanas abiertas y fondos de pantalla animados. Cerrar ventanas que no uses lo reduce.".into(),
            savings_bytes: None,
            action: None,
        });
    }

    // 10. Long uptime.
    if uptime_secs >= 7 * 86400 && b.pressure != MemoryPressure::Normal {
        out.push(Insight {
            id: "uptime".into(),
            severity: Severity::Suggestion,
            title: format!("Llevas {} sin reiniciar", human_duration(uptime_secs)),
            detail: "Reiniciar limpia el swap, la memoria comprimida y procesos que quedaron colgados. Es lo más efectivo cuando el Mac se siente lento.".into(),
            savings_bytes: Some(b.swap_used),
            action: None,
        });
    }

    // 11. Purge only helps with inactive cache; be honest about it.
    if b.pressure != MemoryPressure::Normal && b.cached >= 1 << 30 {
        out.push(Insight {
            id: "purge".into(),
            severity: Severity::Info,
            title: "Liberar memoria inactiva".into(),
            detail: format!(
                "Vacía {} de caché de archivos. Ayuda un rato, pero lo que de verdad libera memoria es cerrar las apps de arriba.",
                fmt_mem(b.cached)
            ),
            savings_bytes: Some(b.cached / 2),
            action: Some(InsightAction::Optimize { action: OptimizeAction::PurgeMemory }),
        });
    }

    // 12. Point to unused apps for the full picture.
    if b.pressure != MemoryPressure::Normal {
        out.push(Insight {
            id: "apps".into(),
            severity: Severity::Info,
            title: "Revisa las apps que no usas".into(),
            detail: "Muchas apps sin uso dejan agentes que arrancan con el Mac y ocupan memoria. La sección Apps te muestra cuáles llevan meses sin abrirse.".into(),
            savings_bytes: None,
            action: Some(InsightAction::OpenPage { page: "apps".into() }),
        });
    }

    // The overall diagnosis always leads; then by severity and potential savings.
    out.sort_by(|a, b| {
        (b.id == "pressure")
            .cmp(&(a.id == "pressure"))
            .then(b.severity.cmp(&a.severity))
            .then(b.savings_bytes.unwrap_or(0).cmp(&a.savings_bytes.unwrap_or(0)))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, kind: AppKind, gb: f64) -> AppMemory {
        AppMemory {
            id: format!("/Applications/{name}.app"),
            name: name.into(),
            bundle_id: Some(format!("com.test.{name}")),
            kind,
            footprint_bytes: (gb * GIB) as u64,
            process_count: 1,
            renderer_count: 0,
            cpu: 0.0,
            pids: vec![1],
            main_pid: Some(1),
            killable: true,
            is_gui: true,
            running_secs: 10 * 3600,
            last_focused_secs_ago: None,
            growth_per_hour: None,
        }
    }

    #[test]
    fn vm_stat_parsing() {
        let s = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free:                                     4096.\n\"Translation faults\":                     123456789.\n";
        let (page, m) = parse_vm_stat(s);
        assert_eq!(page, 16384);
        assert_eq!(m["Pages free"], 4096);
        assert_eq!(m["Translation faults"], 123456789);
    }

    #[test]
    fn insights_under_pressure() {
        let b = MemoryBreakdown {
            total: 24 << 30,
            app: 7 << 30,
            wired: 3 << 30,
            compressed: 9 << 30,
            compressed_original: 19 << 30,
            cached: 3 << 30,
            swap_used: 7 << 30,
            pressure: MemoryPressure::Warning,
            free_percent: 45,
            ..Default::default()
        };
        let mut chrome = app("Chrome", AppKind::Browser, 6.0);
        chrome.renderer_count = 30;
        let mut idle = app("Spotify", AppKind::Electron, 0.8);
        idle.last_focused_secs_ago = Some(3 * 3600);
        let slack = app("Slack", AppKind::Electron, 0.6);
        let mut leaky = app("Leaky", AppKind::App, 2.0);
        leaky.growth_per_hour = Some(500 << 20);
        let docker = app("Docker", AppKind::Virtualization, 4.0);

        let ins = insights(&b, &[chrome, idle, slack, leaky, docker], 3600, 10 * 86400, Some("com.test.Leaky"));
        let ids: Vec<&str> = ins.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ins[0].id, "pressure");
        assert!(ids.contains(&"idle:/Applications/Spotify.app"));
        assert!(ids.contains(&"browser:/Applications/Chrome.app"));
        assert!(ids.contains(&"electron"));
        assert!(ids.contains(&"vm:/Applications/Docker.app"));
        assert!(ids.contains(&"leak:/Applications/Leaky.app"));
        assert!(ids.contains(&"uptime"));
        assert!(!ids.contains(&"idle:/Applications/Leaky.app"), "front app is never idle");
        assert!(score(&b) <= 59);
    }

    #[test]
    fn outermost_bundle() {
        let p = Path::new("/Applications/Google Chrome.app/Contents/Frameworks/X.framework/Helpers/Google Chrome Helper (Renderer).app/Contents/MacOS/x");
        assert_eq!(outermost_app(p).unwrap(), Path::new("/Applications/Google Chrome.app"));
        assert!(outermost_app(Path::new("/usr/sbin/mDNSResponder")).is_none());
    }
}

// ---------------------------------------------------------------------------
// Quitting apps
// ---------------------------------------------------------------------------

/// Quits the app owning `pid` gracefully: AppleScript `quit` by bundle id
/// (lets the app ask to save documents), SIGTERM for loose processes.
/// Only processes of the current user.
pub fn quit_app(pid: u32) -> Result<(), String> {
    let mut sys = System::new();
    let spid = sysinfo::Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[spid]),
        true,
        ProcessRefreshKind::nothing().with_user(UpdateKind::Always).with_exe(UpdateKind::Always),
    );
    let p = sys.process(spid).ok_or("La app ya no está abierta")?;
    if p.user_id().map(|u| **u) != Some(unsafe { libc::getuid() }) {
        return Err("Solo se pueden cerrar apps de tu usuario".into());
    }
    let id = p.exe().and_then(outermost_app).and_then(|b| read_bundle(&b)).and_then(|m| m.id);
    if let Some(id) = id {
        if id == "com.anthropic.claudefordesktop" {
            return Err("No cierro Claude desde aquí: cortaría esta sesión".into());
        }
        // Bundle ids are [A-Za-z0-9.-]; anything else is refused instead of escaped.
        if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_') {
            return Err("Identificador de app no válido".into());
        }
        let out = Command::new("/usr/bin/osascript")
            .args(["-e", &format!("tell application id \"{id}\" to quit")])
            .output()
            .map_err(|e| e.to_string())?;
        return if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) };
    }
    match p.kill_with(sysinfo::Signal::Term) {
        Some(true) => Ok(()),
        _ => Err("No se pudo cerrar el proceso".into()),
    }
}

//! Realistic mock backend used when the UI runs in a plain browser
//! (`trunk serve`). Simulates a generic Apple Silicon MacBook Pro.

use std::cell::RefCell;
use std::collections::HashSet;

use iclear_protocol::*;

use crate::api::sleep;

const KB: u64 = 1_000;
const MB: u64 = 1_000_000;
const GIB: u64 = 1_073_741_824;
const HOME: &str = "/Users/usuario";
const FAN_MIN: f32 = 1200.0;
const FAN_MAX: f32 = 6000.0;

struct Sim {
    helper_installed: bool,
    helper: HelperState,
    fan_rpm: [f32; 2],
    killed: HashSet<u32>,
    quit_apps: HashSet<u32>,
    cleaned: HashSet<String>,
    deleted: HashSet<String>,
    last_preview: Option<CleanupPreview>,
    agents: Vec<LaunchAgent>,
    token_seq: u32,
}

impl Sim {
    fn new() -> Self {
        Self {
            helper_installed: false,
            helper: HelperState {
                protocol_version: HELPER_PROTOCOL_VERSION,
                helper_version: "0.1.0".into(),
                charge_limit: None,
                charging_inhibited: false,
                force_discharge: false,
                manual_fans: vec![],
                supports_charge_limit: false,
                supports_force_discharge: false,
                supports_fan_control: true,
            },
            fan_rpm: [2480.0, 2560.0],
            killed: HashSet::new(),
            quit_apps: HashSet::new(),
            cleaned: HashSet::new(),
            deleted: HashSet::new(),
            last_preview: None,
            agents: initial_agents(),
            token_seq: 0,
        }
    }
}

thread_local! {
    static SIM: RefCell<Sim> = RefCell::new(Sim::new());
    static FAN_TARGETS: RefCell<[Option<f32>; 2]> = const { RefCell::new([None, None]) };
}

fn rnd() -> f64 {
    js_sys::Math::random()
}

fn jitter(amount: f32) -> f32 {
    (rnd() as f32 - 0.5) * 2.0 * amount
}

fn now_s() -> f64 {
    js_sys::Date::now() / 1000.0
}

fn helper_guard() -> Result<(), String> {
    if SIM.with(|s| s.borrow().helper_installed) {
        Ok(())
    } else {
        Err("El asistente no está instalado.".into())
    }
}

// ---------------------------------------------------------------------------
// Machine / metrics
// ---------------------------------------------------------------------------

pub fn machine_info() -> MachineInfo {
    MachineInfo {
        model: "MacBook Pro".into(),
        chip: "Apple M3 Pro".into(),
        os_version: "macOS 26.1".into(),
        hostname: "MacBook-Pro.local".into(),
        cpu_cores: 12,
        mem_total: 24 * GIB,
    }
}

pub fn metrics() -> Metrics {
    metrics_at(now_s())
}

/// Sample at an arbitrary time (used to pre-fill history in preview mode).
pub fn metrics_at(t: f64) -> Metrics {
    let cpu = (17.0 + 9.0 * (t / 9.0).sin() + 7.0 * (t / 3.3).sin().abs() + rnd() * 6.0) as f32;
    let cpu = cpu.clamp(2.0, 100.0);
    let per_core = (0..14)
        .map(|i| {
            let base = if i < 10 { cpu * 1.15 } else { cpu * 0.55 };
            (base + jitter(14.0)).clamp(0.5, 100.0)
        })
        .collect();

    let mem_total = 24 * GIB;
    let mem_used = (21.6 * GIB as f64 + 0.4 * GIB as f64 * (t / 23.0).sin()) as u64;

    let rx = (220.0 * KB as f64 + 180.0 * KB as f64 * (t / 5.0).sin().abs() + rnd() * 900.0 * KB as f64) as u64;
    let tx = (40.0 * KB as f64 + rnd() * 160.0 * KB as f64) as u64;

    let system = SystemSnapshot {
        cpu_total: cpu,
        cpu_per_core: per_core,
        mem_total,
        mem_used,
        swap_total: 8 * GIB,
        swap_used: (5.4 * GIB as f64) as u64,
        load_avg: [2.41 + cpu as f64 / 40.0, 2.87, 3.02],
        uptime_secs: 3 * 86_400 + 5 * 3600 + 17 * 60 + (t as u64 % 3600),
        net_rx_per_sec: rx,
        net_tx_per_sec: tx,
        disks: vec![DiskInfo {
            name: "Macintosh HD".into(),
            mount_point: "/".into(),
            total_bytes: 460_400 * MB,
            available_bytes: 182_600 * MB,
        }],
        process_count: 612,
    };

    let power = 24.6 + jitter(1.5);
    let battery = BatteryInfo {
        percent: 35,
        is_charging: true,
        external_connected: true,
        fully_charged: false,
        cycle_count: 212,
        design_capacity_mah: 6075,
        max_capacity_mah: 5650,
        current_capacity_mah: 1722,
        health_percent: 93.0,
        temperature_c: 31.4 + jitter(0.3),
        voltage_v: 11.87 + jitter(0.03),
        amperage_ma: (power / 11.87 * 1000.0) as i32,
        power_w: power,
        time_remaining_min: Some(74),
        adapter_watts: Some(96),
        adapter_name: Some("Adaptador de corriente USB-C de 96 W".into()),
        not_charging_reason: Some(0),
    };

    // Fans follow CPU load in auto mode, or the manual target.
    let targets = FAN_TARGETS.with(|f| *f.borrow());
    let fans = SIM.with(|s| {
        let mut s = s.borrow_mut();
        (0..2)
            .map(|i| {
                let auto_target = (FAN_MIN + 120.0 + cpu * 45.0 + i as f32 * 60.0).clamp(FAN_MIN, FAN_MAX);
                let target = targets[i].unwrap_or(auto_target);
                let cur = s.fan_rpm[i];
                let next = (cur + (target - cur) * 0.35 + jitter(25.0)).clamp(FAN_MIN * 0.95, FAN_MAX);
                s.fan_rpm[i] = next;
                FanInfo {
                    index: i as u8,
                    actual_rpm: next,
                    min_rpm: FAN_MIN,
                    max_rpm: FAN_MAX,
                    target_rpm: Some(target),
                    mode: if targets[i].is_some() { FanMode::Manual } else { FanMode::Auto },
                }
            })
            .collect::<Vec<_>>()
    });

    let cpu_t = 41.0 + cpu * 0.42;
    let mut temps = Vec::new();
    let mut push = |key: &str, label: &str, group: &str, c: f32| {
        temps.push(TempSensor { key: key.into(), label: label.into(), group: group.into(), celsius: c });
    };
    for i in 0..6 {
        push(&format!("Tp0{i}"), &format!("CPU P-core · Tp0{i}"), "CPU", cpu_t + 2.0 + jitter(2.5));
    }
    for i in 0..2 {
        push(&format!("Te0{i}"), &format!("CPU E-core · Te0{i}"), "CPU", cpu_t - 4.0 + jitter(1.5));
    }
    for i in 0..3 {
        push(&format!("Tg0{i}"), &format!("GPU · Tg0{i}"), "GPU", cpu_t - 3.0 + jitter(1.8));
    }
    push("Tm0P", "Memoria · Tm0P", "Memoria", 44.2 + jitter(0.6));
    push("Tm1P", "Memoria · Tm1P", "Memoria", 43.6 + jitter(0.6));
    push("TB0T", "Batería · TB0T", "Batería", 31.4 + jitter(0.3));
    push("TB1T", "Batería · TB1T", "Batería", 31.0 + jitter(0.3));
    push("TH0x", "SSD · TH0x", "SSD", 38.5 + jitter(0.5));
    push("TW0P", "Wi-Fi · TW0P", "Wi-Fi", 42.8 + jitter(0.4));
    push("Ts0P", "Chasis · Ts0P", "Chasis", 30.2 + jitter(0.3));
    push("Ts1P", "Chasis · Ts1P", "Chasis", 31.6 + jitter(0.3));
    push("TPD0", "Energía · TPD0", "Energía", 39.7 + jitter(0.5));
    push("TaLP", "Ambiente · TaLP", "Ambiente", 36.1 + jitter(0.6));
    push("TaRF", "Ambiente · TaRF", "Ambiente", 35.4 + jitter(0.6));

    let avg = |g: &str| {
        let v: Vec<f32> = temps.iter().filter(|t| t.group == g).map(|t| t.celsius).collect();
        (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32)
    };
    let temp_summary = TempSummary {
        cpu_avg: avg("CPU"),
        cpu_max: temps.iter().filter(|t| t.group == "CPU").map(|t| t.celsius).reduce(f32::max),
        gpu_avg: avg("GPU"),
        battery: avg("Batería"),
        ssd: avg("SSD"),
    };

    Metrics {
        timestamp_ms: js_sys::Date::now() as u64,
        system,
        battery: Some(battery),
        fans,
        temps,
        temp_summary,
    }
}

// ---------------------------------------------------------------------------
// Processes
// ---------------------------------------------------------------------------

const PROCS: &[(u32, &str, &str, bool, f32, u64)] = &[
    (0, "kernel_task", "root", false, 6.1, 2_400),
    (1, "launchd", "root", false, 0.3, 38),
    (412, "WindowServer", "_windowserver", false, 9.8, 1_350),
    (533, "mds_stores", "root", false, 2.4, 410),
    (601, "Finder", "usuario", true, 0.6, 290),
    (618, "Dock", "usuario", true, 0.2, 120),
    (640, "Control Center", "usuario", true, 0.1, 95),
    (1022, "Safari", "usuario", true, 3.2, 820),
    (1031, "Safari Web Content", "usuario", true, 5.8, 1_640),
    (1045, "Google Chrome", "usuario", true, 4.1, 960),
    (1046, "Google Chrome Helper (Renderer)", "usuario", true, 11.4, 2_150),
    (1047, "Google Chrome Helper (GPU)", "usuario", true, 3.9, 610),
    (1102, "Code", "usuario", true, 2.1, 740),
    (1103, "Code Helper (Plugin)", "usuario", true, 6.6, 1_280),
    (1150, "rust-analyzer", "usuario", true, 14.2, 3_900),
    (1188, "cargo", "usuario", true, 0.0, 85),
    (1201, "Slack", "usuario", true, 1.4, 520),
    (1202, "Slack Helper (Renderer)", "usuario", true, 2.2, 690),
    (1240, "Spotify", "usuario", true, 1.9, 410),
    (1290, "Terminal", "usuario", true, 0.4, 180),
    (1301, "Xcode", "usuario", true, 0.8, 1_920),
    (1333, "Simulator", "usuario", true, 1.1, 760),
    (1360, "Docker Desktop", "usuario", true, 2.7, 1_100),
    (1361, "com.docker.backend", "usuario", true, 3.3, 2_800),
    (1402, "Mail", "usuario", true, 0.3, 260),
    (1430, "Notas", "usuario", true, 0.1, 150),
    (1455, "iclear", "usuario", true, 1.2, 140),
    (1490, "photoanalysisd", "usuario", true, 0.9, 320),
    (1520, "bird", "usuario", true, 0.2, 60),
    (1561, "cloudd", "usuario", true, 0.4, 110),
    (1602, "coreaudiod", "_coreaudiod", false, 0.7, 48),
    (1633, "bluetoothd", "root", false, 0.3, 36),
    (1650, "airportd", "root", false, 0.1, 22),
    (1688, "softwareupdated", "_softwareupdate", false, 0.0, 44),
    (1701, "Webex", "usuario", true, 0.0, 380),
    (1742, "Figma", "usuario", true, 3.6, 1_010),
    (1743, "Figma Helper (Renderer)", "usuario", true, 4.8, 1_420),
    (1790, "node", "usuario", true, 7.4, 650),
];

pub fn processes() -> Vec<ProcessInfo> {
    let killed = SIM.with(|s| s.borrow().killed.clone());
    let mut list: Vec<ProcessInfo> = PROCS
        .iter()
        .filter(|p| !killed.contains(&p.0))
        .map(|&(pid, name, user, killable, cpu, mem_mb)| ProcessInfo {
            pid,
            name: name.into(),
            cpu: (cpu * (0.6 + rnd() as f32 * 0.8)).max(0.0),
            memory: ((mem_mb as f64) * (0.97 + rnd() * 0.06) * 1_048_576.0) as u64,
            user: Some(user.into()),
            killable,
        })
        .collect();
    list.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
    list
}

pub async fn kill_process(pid: u32, _force: bool) -> Result<(), String> {
    sleep(250).await;
    match PROCS.iter().find(|p| p.0 == pid) {
        Some(p) if !p.3 => Err("No se puede terminar un proceso del sistema.".into()),
        Some(_) => {
            SIM.with(|s| s.borrow_mut().killed.insert(pid));
            Ok(())
        }
        None => Err(format!("No existe el proceso {pid}.")),
    }
}

// ---------------------------------------------------------------------------
// Cleanup
// ---------------------------------------------------------------------------

struct MockCategory {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    roots: &'static [&'static str],
    size: u64,
    items: &'static [&'static str],
    risk: Risk,
    requires_helper: bool,
    selected: bool,
}

const CATEGORIES: &[MockCategory] = &[
    MockCategory {
        id: "user_caches",
        name: "Cachés de usuario",
        description: "Archivos temporales que las apps regeneran automáticamente.",
        roots: &["~/Library/Caches"],
        size: 3_420 * MB,
        items: &["com.spotify.client", "com.apple.Safari", "Google", "com.microsoft.VSCode", "Homebrew", "com.figma.Desktop"],
        risk: Risk::Safe,
        requires_helper: false,
        selected: true,
    },
    MockCategory {
        id: "logs",
        name: "Registros",
        description: "Logs de apps y reportes de fallos antiguos.",
        roots: &["~/Library/Logs"],
        size: 412 * MB,
        items: &["DiagnosticReports", "JetBrains", "Docker", "CoreSimulator"],
        risk: Risk::Safe,
        requires_helper: false,
        selected: true,
    },
    MockCategory {
        id: "trash",
        name: "Papelera",
        description: "Elementos que ya enviaste a la papelera.",
        roots: &["~/.Trash"],
        size: 1_870 * MB,
        items: &["Captura de pantalla 2026-10-02.png", "instalador-viejo.dmg", "borrador.key"],
        risk: Risk::Safe,
        requires_helper: false,
        selected: true,
    },
    MockCategory {
        id: "browser_caches",
        name: "Cachés de navegadores",
        description: "Caché de Safari, Chrome y Firefox. No borra contraseñas ni historial.",
        roots: &["~/Library/Caches/Google/Chrome", "~/Library/Caches/com.apple.Safari"],
        size: 1_240 * MB,
        items: &["Chrome/Default/Cache", "Chrome/Default/Code Cache", "Safari/WebKitCache"],
        risk: Risk::Safe,
        requires_helper: false,
        selected: true,
    },
    MockCategory {
        id: "xcode",
        name: "Xcode DerivedData",
        description: "Compilaciones intermedias de Xcode. Se regeneran al compilar de nuevo.",
        roots: &["~/Library/Developer/Xcode/DerivedData"],
        size: 18_400 * MB,
        items: &["iclear-abxhqk", "Runner-fgtqwz", "MiApp-cdpsle", "ModuleCache.noindex"],
        risk: Risk::Moderate,
        requires_helper: false,
        selected: false,
    },
    MockCategory {
        id: "dev_caches",
        name: "Cachés de desarrollo",
        description: "Cargo, npm, pnpm, pip y Gradle. Se vuelven a descargar si se necesitan.",
        roots: &["~/.cargo/registry", "~/.npm/_cacache", "~/Library/Caches/pnpm"],
        size: 6_150 * MB,
        items: &[".cargo/registry/cache", ".npm/_cacache", "Library/Caches/pnpm", ".gradle/caches"],
        risk: Risk::Moderate,
        requires_helper: false,
        selected: false,
    },
    MockCategory {
        id: "old_downloads",
        name: "Descargas antiguas",
        description: "Archivos en Descargas sin abrir hace más de 90 días.",
        roots: &["~/Downloads"],
        size: 4_630 * MB,
        items: &["Xcode_16.dmg", "dataset-2025.zip", "presentacion-final.pdf", "video-demo.mov"],
        risk: Risk::Review,
        requires_helper: false,
        selected: false,
    },
    MockCategory {
        id: "ios_backups",
        name: "Respaldos de iOS",
        description: "Copias locales de iPhone y iPad. Asegúrate de tener respaldo en iCloud.",
        roots: &["~/Library/Application Support/MobileSync/Backup"],
        size: 22_800 * MB,
        items: &["00008110-001A2B3C4D5E", "00008030-000F1E2D3C4B"],
        risk: Risk::Review,
        requires_helper: false,
        selected: false,
    },
    MockCategory {
        id: "system_logs",
        name: "Registros del sistema",
        description: "Logs en /private/var/log y /Library/Logs. Requiere el asistente.",
        roots: &["/private/var/log", "/Library/Logs"],
        size: 690 * MB,
        items: &["/private/var/log/asl", "/Library/Logs/DiagnosticReports"],
        risk: Risk::Safe,
        requires_helper: true,
        selected: false,
    },
];

fn expand(root: &str) -> String {
    root.replacen('~', HOME, 1)
}

fn category_items(c: &MockCategory) -> Vec<PreviewItem> {
    let n = c.items.len() as u64;
    let weights: u64 = (1..=n).sum();
    c.items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let path = if item.starts_with('/') {
                item.to_string()
            } else if item.starts_with('.') || item.starts_with("Library/") {
                format!("{HOME}/{item}")
            } else {
                format!("{}/{}", expand(c.roots[0]), item)
            };
            PreviewItem {
                path,
                size_bytes: c.size * (n - i as u64) / weights,
                is_dir: ![".png", ".dmg", ".key", ".zip", ".pdf", ".mov"].iter().any(|ext| item.ends_with(ext)),
                category_id: c.id.into(),
                modified_days_ago: Some(3 + (i as u64 * 37) % 200),
            }
        })
        .collect()
}

pub async fn cleanup_categories() -> Vec<CleanupCategory> {
    sleep(1400).await;
    let cleaned = SIM.with(|s| s.borrow().cleaned.clone());
    CATEGORIES
        .iter()
        .map(|c| {
            let done = cleaned.contains(c.id);
            CleanupCategory {
                id: c.id.into(),
                name: c.name.into(),
                description: c.description.into(),
                roots: c.roots.iter().map(|r| r.to_string()).collect(),
                size_bytes: if done { 0 } else { c.size },
                item_count: if done { 0 } else { c.items.len() },
                risk: c.risk,
                requires_helper: c.requires_helper,
                selected_by_default: c.selected,
            }
        })
        .collect()
}

fn new_preview(items: Vec<PreviewItem>) -> CleanupPreview {
    let token = SIM.with(|s| {
        let mut s = s.borrow_mut();
        s.token_seq += 1;
        format!("mock-{}", s.token_seq)
    });
    let preview = CleanupPreview {
        token,
        total_bytes: items.iter().map(|i| i.size_bytes).sum(),
        items,
    };
    SIM.with(|s| s.borrow_mut().last_preview = Some(preview.clone()));
    preview
}

pub async fn preview_cleanup(ids: &[String]) -> CleanupPreview {
    sleep(700).await;
    let cleaned = SIM.with(|s| s.borrow().cleaned.clone());
    let items = CATEGORIES
        .iter()
        .filter(|c| ids.iter().any(|id| id == c.id) && !cleaned.contains(c.id))
        .flat_map(category_items)
        .collect();
    new_preview(items)
}

pub async fn preview_paths(paths: &[String]) -> CleanupPreview {
    sleep(400).await;
    let files = all_large_files();
    let items = paths
        .iter()
        .map(|p| PreviewItem {
            path: p.clone(),
            size_bytes: files
                .iter()
                .find(|f| &f.path == p)
                .map(|f| f.size_bytes)
                .or_else(|| {
                    DUPLICATES
                        .iter()
                        .find(|d| d.2.iter().any(|x| expand(x) == *p))
                        .map(|d| d.1)
                })
                .unwrap_or(120 * MB),
            is_dir: false,
            category_id: "manual".into(),
            modified_days_ago: Some(41),
        })
        .collect();
    new_preview(items)
}

pub async fn execute_cleanup(token: &str, paths: &[String]) -> Result<DeleteReport, String> {
    sleep(900).await;
    SIM.with(|s| {
        let mut s = s.borrow_mut();
        let preview = s.last_preview.take().filter(|p| p.token == token).ok_or_else(|| {
            "La vista previa expiró. Vuelve a revisar antes de borrar.".to_string()
        })?;
        let chosen: Vec<&PreviewItem> = preview.items.iter().filter(|i| paths.contains(&i.path)).collect();
        let freed = chosen.iter().map(|i| i.size_bytes).sum();
        for item in &chosen {
            s.deleted.insert(item.path.clone());
        }
        // A category counts as cleaned when all of its items were deleted.
        for c in CATEGORIES {
            let items = category_items(c);
            if items.iter().all(|i| s.deleted.contains(&i.path)) {
                s.cleaned.insert(c.id.to_string());
            }
        }
        Ok(DeleteReport { deleted: chosen.len(), freed_bytes: freed, errors: vec![] })
    })
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

fn hash(s: &str) -> u64 {
    s.bytes().fold(1469598103934665603u64, |h, b| (h ^ b as u64).wrapping_mul(1099511628211))
}

pub async fn disk_usage(path: Option<&str>) -> UsageReport {
    sleep(650).await;
    let root = path.unwrap_or(HOME).to_string();
    let known: Option<&[(&str, u64, bool)]> = match root.strip_prefix(HOME) {
        Some("") => Some(&[
            ("Library", 92_300 * MB, true),
            ("Developer", 64_800 * MB, true),
            ("Documents", 38_200 * MB, true),
            ("Downloads", 21_400 * MB, true),
            ("Movies", 17_100 * MB, true),
            ("Pictures", 12_600 * MB, true),
            (".cargo", 6_200 * MB, true),
            ("Music", 4_100 * MB, true),
            (".rustup", 3_300 * MB, true),
            ("Desktop", 2_150 * MB, true),
            (".npm", 1_400 * MB, true),
            (".zsh_history", 2 * MB, false),
        ]),
        Some("/Library") => Some(&[
            ("Developer", 31_400 * MB, true),
            ("Application Support", 22_100 * MB, true),
            ("Containers", 14_700 * MB, true),
            ("Caches", 9_600 * MB, true),
            ("Group Containers", 7_200 * MB, true),
            ("Mail", 4_300 * MB, true),
            ("Messages", 2_100 * MB, true),
            ("Logs", 420 * MB, true),
        ]),
        _ => None,
    };
    let children: Vec<DirUsage> = match known {
        Some(list) => list
            .iter()
            .map(|(name, size, is_dir)| DirUsage {
                path: format!("{root}/{name}"),
                name: name.to_string(),
                size_bytes: *size,
                is_dir: *is_dir,
            })
            .collect(),
        None => {
            const NAMES: &[&str] = &[
                "build", "cache", "data", "assets", "proyectos", "exportaciones", "node_modules",
                "fotos-2025", "respaldo", "tmp", "videos", "notas.md", "archivo.zip", "informe.pdf",
            ];
            let h = hash(&root);
            let count = 4 + (h % 6) as usize;
            let mut v: Vec<DirUsage> = (0..count)
                .map(|i| {
                    let name = NAMES[((h >> (i * 4)) as usize + i) % NAMES.len()];
                    let size = (h.rotate_left(i as u32 * 7) % 9_000 + 40) * MB / (i as u64 + 1);
                    DirUsage {
                        path: format!("{root}/{name}"),
                        name: name.into(),
                        size_bytes: size,
                        is_dir: !name.contains('.'),
                    }
                })
                .collect();
            v.dedup_by(|a, b| a.name == b.name);
            v
        }
    };
    let mut children = children;
    children.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    UsageReport {
        total_bytes: children.iter().map(|c| c.size_bytes).sum(),
        root,
        children,
        skipped_errors: if path.is_none() { 3 } else { 0 },
    }
}

fn all_large_files() -> Vec<LargeFile> {
    const FILES: &[(&str, u64, u64)] = &[
        ("Movies/Grabación de pantalla 2026-09-14.mov", 8_400, 62),
        ("Downloads/Xcode_16.dmg", 7_100, 210),
        ("Developer/vm/ubuntu-24.04.qcow2", 6_300, 33),
        ("Downloads/dataset-2025.zip", 3_200, 145),
        ("Documents/Proyectos/render-final.mp4", 2_650, 18),
        ("Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw", 2_400, 1),
        ("Movies/vacaciones-4k.mov", 1_980, 400),
        ("Downloads/macOS-instalador.pkg", 1_420, 380),
        ("Pictures/Biblioteca.photoslibrary/originals/IMG_4021.HEIC.mov", 980, 120),
        ("Documents/respaldo-notion.zip", 760, 95),
        ("Music/podcast-crudo.wav", 540, 260),
        ("Desktop/presentacion-cliente.key", 410, 7),
        ("Downloads/figma-export.zip", 310, 51),
        ("Documents/tesis/datos.csv", 190, 600),
        ("Developer/iclear/target/debug/iclear", 140, 0),
        ("Downloads/factura-octubre.pdf", 62, 30),
    ];
    let deleted = SIM.with(|s| s.borrow().deleted.clone());
    FILES
        .iter()
        .map(|(p, mb, days)| LargeFile {
            path: format!("{HOME}/{p}"),
            size_bytes: mb * MB,
            modified_days_ago: Some(*days),
        })
        .filter(|f| !deleted.contains(&f.path))
        .collect()
}

pub async fn large_files(min_mb: u64, limit: usize) -> Vec<LargeFile> {
    sleep(1600).await;
    all_large_files()
        .into_iter()
        .filter(|f| f.size_bytes >= min_mb * MB)
        .take(limit)
        .collect()
}

const DUPLICATES: &[(&str, u64, &[&str])] = &[
    (
        "9f2c4a71e0b3d5c8",
        1_420 * MB,
        &["~/Downloads/macOS-instalador.pkg", "~/Documents/Instaladores/macOS-instalador.pkg"],
    ),
    (
        "41ad09be7c2f6e13",
        640 * MB,
        &["~/Movies/clip-boda.mov", "~/Desktop/clip-boda.mov", "~/Documents/Respaldo/clip-boda copia.mov"],
    ),
    (
        "c07e5b2d81fa4490",
        210 * MB,
        &["~/Downloads/fotos-viaje.zip", "~/Downloads/fotos-viaje (1).zip"],
    ),
    (
        "7b3e90d4aa1c5f62",
        58 * MB,
        &["~/Documents/contrato-final.pdf", "~/Desktop/contrato-final.pdf"],
    ),
];

pub async fn find_duplicates(min_mb: u64) -> Vec<DuplicateGroup> {
    sleep(2200).await;
    let deleted = SIM.with(|s| s.borrow().deleted.clone());
    DUPLICATES
        .iter()
        .filter(|d| d.1 >= min_mb * MB)
        .map(|(h, size, paths)| DuplicateGroup {
            hash: h.to_string(),
            size_bytes: *size,
            paths: paths.iter().map(|p| expand(p)).filter(|p| !deleted.contains(p)).collect(),
        })
        .filter(|g| g.paths.len() > 1)
        .collect()
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

pub fn helper_status() -> HelperStatus {
    SIM.with(|s| {
        let s = s.borrow();
        HelperStatus {
            installed: s.helper_installed,
            reachable: s.helper_installed,
            state: s.helper_installed.then(|| s.helper.clone()),
            error: (!s.helper_installed).then(|| "No se encontró el socket del asistente.".into()),
        }
    })
}

pub async fn install_helper() -> HelperStatus {
    sleep(1300).await;
    SIM.with(|s| s.borrow_mut().helper_installed = true);
    helper_status()
}

pub async fn uninstall_helper() -> HelperStatus {
    sleep(800).await;
    SIM.with(|s| {
        let mut s = s.borrow_mut();
        s.helper_installed = false;
        s.helper.charge_limit = None;
        s.helper.force_discharge = false;
        s.helper.manual_fans.clear();
    });
    FAN_TARGETS.with(|f| *f.borrow_mut() = [None, None]);
    helper_status()
}

fn state() -> HelperState {
    SIM.with(|s| s.borrow().helper.clone())
}

pub async fn set_fan_auto(fan: u8) -> Result<HelperState, String> {
    helper_guard()?;
    sleep(200).await;
    FAN_TARGETS.with(|f| f.borrow_mut()[(fan as usize).min(1)] = None);
    SIM.with(|s| s.borrow_mut().helper.manual_fans.retain(|f| *f != fan));
    Ok(state())
}

pub async fn set_fan_rpm(fan: u8, rpm: f32) -> Result<HelperState, String> {
    helper_guard()?;
    sleep(200).await;
    FAN_TARGETS.with(|f| f.borrow_mut()[(fan as usize).min(1)] = Some(rpm.clamp(FAN_MIN, FAN_MAX)));
    SIM.with(|s| {
        let mut s = s.borrow_mut();
        if !s.helper.manual_fans.contains(&fan) {
            s.helper.manual_fans.push(fan);
        }
    });
    Ok(state())
}

pub async fn set_charge_limit(percent: Option<u8>) -> Result<HelperState, String> {
    helper_guard()?;
    sleep(250).await;
    SIM.with(|s| {
        let mut s = s.borrow_mut();
        s.helper.charge_limit = percent;
        s.helper.charging_inhibited = percent.is_some_and(|p| p <= 35);
    });
    Ok(state())
}

pub async fn set_force_discharge(enabled: bool) -> Result<HelperState, String> {
    helper_guard()?;
    sleep(250).await;
    SIM.with(|s| s.borrow_mut().helper.force_discharge = enabled);
    Ok(state())
}

pub async fn run_optimization(action: OptimizeAction) -> Result<String, String> {
    helper_guard()?;
    sleep(1500).await;
    Ok(match action {
        OptimizeAction::PurgeMemory => "Memoria inactiva liberada (≈ 2,4 GB).",
        OptimizeAction::FlushDns => "Caché DNS vaciada.",
        OptimizeAction::ThinLocalSnapshots => "Se eliminaron 3 snapshots locales (≈ 11,2 GB).",
        OptimizeAction::ReindexSpotlight => "Spotlight reindexará el disco en segundo plano.",
        OptimizeAction::CleanSystemCaches => "Cachés del sistema eliminadas (≈ 1,1 GB).",
    }
    .into())
}

// ---------------------------------------------------------------------------
// Launch agents
// ---------------------------------------------------------------------------

fn initial_agents() -> Vec<LaunchAgent> {
    let a = |label: &str, scope: &str, loaded: bool, disabled: bool, program: &str| {
        let dir = match scope {
            "user" => format!("{HOME}/Library/LaunchAgents"),
            "global-agent" => "/Library/LaunchAgents".into(),
            _ => "/Library/LaunchDaemons".into(),
        };
        LaunchAgent {
            label: label.into(),
            path: format!("{dir}/{label}.plist"),
            scope: scope.into(),
            loaded,
            disabled,
            program: Some(program.into()),
        }
    };
    vec![
        a("com.spotify.webhelper", "user", true, false, "/Applications/Spotify.app/Contents/MacOS/SpotifyWebHelper"),
        a("com.google.keystone.agent", "user", true, false, "~/Library/Google/GoogleSoftwareUpdate/GoogleSoftwareUpdateAgent"),
        a("com.dropbox.DropboxMacUpdate.agent", "user", false, true, "~/Library/Dropbox/DropboxMacUpdate.app"),
        a("homebrew.mxcl.postgresql@16", "user", true, false, "/opt/homebrew/opt/postgresql@16/bin/postgres"),
        a("com.adobe.AdobeCreativeCloud", "global-agent", true, false, "/Applications/Utilities/Adobe Creative Cloud/ACC/Creative Cloud.app"),
        a("com.microsoft.update.agent", "global-agent", true, false, "/Library/Application Support/Microsoft/MAU2.0/Microsoft AutoUpdate.app"),
        a("com.docker.vmnetd", "global-daemon", true, false, "/Library/PrivilegedHelperTools/com.docker.vmnetd"),
        a("com.iclear.helper", "global-daemon", false, false, "/Library/PrivilegedHelperTools/com.iclear.helper"),
        a("org.cups.cupsd.local", "global-daemon", false, true, "/usr/sbin/cupsd"),
    ]
}

pub async fn launch_agents() -> Vec<LaunchAgent> {
    sleep(500).await;
    let installed = SIM.with(|s| s.borrow().helper_installed);
    SIM.with(|s| {
        s.borrow()
            .agents
            .iter()
            .cloned()
            .map(|mut a| {
                if a.label == HELPER_LABEL {
                    a.loaded = installed;
                }
                a
            })
            .collect()
    })
}

pub async fn set_launch_agent_enabled(path: &str, enabled: bool) -> Result<(), String> {
    sleep(300).await;
    SIM.with(|s| {
        let mut s = s.borrow_mut();
        match s.agents.iter_mut().find(|a| a.path == path) {
            Some(a) if a.scope == "global-daemon" => Err("Los daemons del sistema son de solo lectura.".into()),
            Some(a) => {
                a.disabled = !enabled;
                a.loaded = enabled;
                Ok(())
            }
            None => Err("No se encontró el elemento.".into()),
        }
    })
}

// ---------------------------------------------------------------------------
// Memory intelligence
// ---------------------------------------------------------------------------

fn gib(x: f64) -> u64 {
    (x * GIB as f64) as u64
}

const MIB: u64 = 1_048_576;

/// (name, bundle id, kind, footprint MiB, procs, renderers, cpu, main pid,
///  killable, gui, last focused secs ago, growth MiB/h)
#[allow(clippy::type_complexity)]
const MEM_APPS: &[(&str, &str, AppKind, u64, usize, usize, f32, u32, bool, bool, Option<u64>, Option<i64>)] = &[
    ("Google Chrome", "com.google.Chrome", AppKind::Browser, 6_350, 44, 38, 18.4, 812, true, true, Some(40), Some(120)),
    ("Docker", "com.docker.docker", AppKind::Virtualization, 4_096, 7, 0, 3.1, 905, true, true, Some(7_400), Some(12)),
    ("Xcode", "com.apple.dt.Xcode", AppKind::Developer, 2_360, 5, 0, 1.8, 1_120, true, true, Some(11_200), Some(-30)),
    ("Visual Studio Code", "com.microsoft.VSCode", AppKind::Electron, 1_840, 16, 9, 6.2, 1_310, true, true, Some(620), Some(85)),
    ("Claude", "com.anthropic.claudefordesktop", AppKind::Electron, 1_430, 9, 6, 4.4, 1_402, true, true, Some(0), Some(40)),
    ("rust-analyzer", "", AppKind::Developer, 1_310, 1, 0, 12.6, 1_350, true, false, None, Some(520)),
    ("Slack", "com.tinyspeck.slackmacgap", AppKind::Electron, 1_150, 7, 4, 0.9, 1_455, true, true, Some(4_900), Some(360)),
    ("WindowServer", "", AppKind::System, 980, 1, 0, 7.9, 151, false, false, None, None),
    ("Figma", "com.figma.Desktop", AppKind::Electron, 910, 6, 3, 0.4, 1_501, true, true, Some(14_600), Some(15)),
    ("Notion", "notion.id", AppKind::Electron, 720, 5, 2, 0.2, 1_533, true, true, Some(18_300), Some(8)),
    ("node", "", AppKind::Developer, 660, 3, 0, 2.7, 1_790, true, false, None, Some(30)),
    ("Spotify", "com.spotify.client", AppKind::Electron, 630, 5, 2, 1.2, 1_560, true, true, Some(5_600), Some(22)),
    ("Dropbox", "com.getdropbox.dropbox", AppKind::Background, 490, 3, 0, 0.6, 1_610, true, false, None, Some(4)),
    ("photoanalysisd", "", AppKind::Background, 430, 1, 0, 0.0, 702, false, false, None, None),
    ("Mail", "com.apple.mail", AppKind::App, 385, 2, 0, 0.1, 1_640, true, true, Some(9_100), Some(3)),
    ("mds_stores", "", AppKind::System, 320, 3, 0, 1.4, 401, false, false, None, None),
    ("Finder", "com.apple.finder", AppKind::System, 215, 1, 0, 0.3, 610, false, true, Some(300), None),
    ("Raycast", "com.raycast.macos", AppKind::App, 190, 2, 0, 0.2, 1_680, true, true, Some(1_250), Some(2)),
    ("Creative Cloud", "com.adobe.acc.AdobeCreativeCloud", AppKind::Background, 176, 3, 0, 0.3, 1_701, true, false, None, Some(1)),
    ("Messages", "com.apple.MobileSMS", AppKind::App, 152, 1, 0, 0.0, 1_720, true, true, Some(21_000), None),
    ("Terminal", "com.apple.Terminal", AppKind::Developer, 128, 2, 0, 0.4, 1_730, true, true, Some(90), None),
    ("cloudd", "", AppKind::System, 96, 1, 0, 0.1, 455, false, false, None, None),
    ("iclear", "com.iclear.app", AppKind::App, 92, 2, 0, 1.6, 1_750, false, true, Some(0), None),
    ("Spotlight", "com.apple.Spotlight", AppKind::System, 88, 1, 0, 0.0, 620, false, false, None, None),
    ("coreaudiod", "", AppKind::System, 48, 1, 0, 0.7, 1_602, false, false, None, None),
    ("bluetoothd", "", AppKind::System, 31, 1, 0, 0.1, 380, false, false, None, None),
    ("Dropbox Helper", "com.getdropbox.dropbox.helper", AppKind::Background, 64, 1, 0, 0.0, 1_770, true, false, None, None),
];

pub fn memory_report() -> MemoryReport {
    let t = now_s();
    let quit = SIM.with(|s| s.borrow().quit_apps.clone());
    // Observation window grows from page load (shows the "learning" hint first).
    let observed = OBSERVED_FROM.with(|o| (t - *o.get_or_init(|| t)) as u64);

    let apps: Vec<AppMemory> = MEM_APPS
        .iter()
        .filter(|a| !quit.contains(&a.7))
        .map(|&(name, bundle, kind, mib, procs, renderers, cpu, pid, killable, gui, focused, growth)| {
            let wobble = 1.0 + 0.012 * ((t / 7.0) + pid as f64).sin();
            AppMemory {
                id: if bundle.is_empty() { name.into() } else { format!("/Applications/{name}.app") },
                name: name.into(),
                bundle_id: (!bundle.is_empty()).then(|| bundle.into()),
                kind,
                footprint_bytes: (mib as f64 * MIB as f64 * wobble) as u64,
                process_count: procs,
                renderer_count: renderers,
                cpu: (cpu * (0.7 + rnd() as f32 * 0.6)).max(0.0),
                pids: (0..procs as u32).map(|i| pid + i).collect(),
                main_pid: Some(pid),
                killable,
                is_gui: gui,
                running_secs: 3 * 3600 + pid as u64 * 7,
                last_focused_secs_ago: focused.map(|f| f + (observed % 60)),
                growth_per_hour: if observed < 120 { None } else { growth.map(|g| g * MIB as i64) },
            }
        })
        .collect();

    // Quitting big apps relieves the compressor and swap.
    let freed: u64 = MEM_APPS
        .iter()
        .filter(|a| quit.contains(&a.7))
        .map(|a| a.3 * MIB)
        .sum();
    let relief = (freed as f64 / gib(12.0) as f64).min(0.9);
    let total = 24 * GIB;
    let wired = gib(3.05);
    let compressed = gib(9.1 * (1.0 - relief)) + gib(0.05 * (t / 11.0).sin());
    let compressed_original = gib(19.7 * (1.0 - relief));
    let cached = gib(0.36);
    let app = gib(11.3 - 2.0 * relief);
    let free = total.saturating_sub(app + wired + compressed + cached);
    let swap_used = gib(5.4 * (1.0 - relief * 0.6));
    let pressure = match relief {
        r if r > 0.55 => MemoryPressure::Normal,
        _ => MemoryPressure::Warning,
    };
    let score = (42.0 + relief * 55.0) as u8;

    let has = |pid: u32| !quit.contains(&pid);
    let mut insights = Vec::new();
    if has(812) {
        insights.push(Insight {
            id: "quit-chrome".into(),
            severity: Severity::Critical,
            title: "Google Chrome ocupa 6,2 GB con 38 pestañas".into(),
            detail: "Es la app que más memoria usa y la principal causa de la compresión y el swap. Cerrar pestañas que no usas o reiniciar Chrome devuelve gran parte.".into(),
            savings_bytes: Some(gib(4.1)),
            action: Some(InsightAction::QuitApp { app_id: "/Applications/Google Chrome.app".into(), main_pid: 812 }),
        });
    }
    if has(905) {
        insights.push(Insight {
            id: "quit-docker".into(),
            severity: Severity::Warning,
            title: "Docker reserva 4 GB y no lo usas hace 2 h".into(),
            detail: "La máquina virtual de Docker Desktop mantiene toda su memoria aunque no tengas contenedores activos.".into(),
            savings_bytes: Some(gib(4.0)),
            action: Some(InsightAction::QuitApp { app_id: "/Applications/Docker.app".into(), main_pid: 905 }),
        });
    }
    if has(1455) {
        insights.push(Insight {
            id: "leak-slack".into(),
            severity: Severity::Warning,
            title: "Slack crece 360 MB por hora".into(),
            detail: "Parece una fuga de memoria. Al reiniciarlo vuelve a ocupar unos 400 MB.".into(),
            savings_bytes: Some(gib(0.7)),
            action: Some(InsightAction::QuitApp { app_id: "/Applications/Slack.app".into(), main_pid: 1455 }),
        });
    }
    insights.push(Insight {
        id: "purge".into(),
        severity: Severity::Suggestion,
        title: "Liberar memoria inactiva".into(),
        detail: "Purga la memoria inactiva que macOS aún no recicla. Úsalo después de cerrar apps grandes.".into(),
        savings_bytes: Some(gib(2.1)),
        action: Some(InsightAction::Optimize { action: OptimizeAction::PurgeMemory }),
    });
    insights.push(Insight {
        id: "login-apps".into(),
        severity: Severity::Suggestion,
        title: "4 apps que no usas se abren al iniciar".into(),
        detail: "Dropbox, Microsoft Teams, Steam y Creative Cloud dejan procesos en segundo plano desde que enciendes el Mac.".into(),
        savings_bytes: Some(380 * MIB),
        action: Some(InsightAction::OpenPage { page: "apps".into() }),
    });
    insights.push(Insight {
        id: "cache-ok".into(),
        severity: Severity::Info,
        title: "La caché de archivos no es un problema".into(),
        detail: "macOS usa la RAM libre como caché de archivos y la entrega al instante cuando una app la necesita.".into(),
        savings_bytes: None,
        action: None,
    });

    MemoryReport {
        breakdown: MemoryBreakdown {
            total,
            app,
            wired,
            compressed,
            compressed_original,
            cached,
            free,
            swap_used,
            swap_total: 8 * GIB,
            pressure,
            free_percent: (18.0 + relief * 40.0) as u8,
            swapouts_bytes: gib(41.0),
        },
        apps,
        insights,
        observed_secs: observed,
        score,
    }
}

thread_local! {
    static OBSERVED_FROM: std::cell::OnceCell<f64> = const { std::cell::OnceCell::new() };
}

pub async fn quit_app(main_pid: u32) -> Result<(), String> {
    sleep(600).await;
    match MEM_APPS.iter().find(|a| a.7 == main_pid) {
        Some(a) if !a.8 => Err(format!("{} es parte del sistema y no se puede cerrar.", a.0)),
        Some(_) => {
            SIM.with(|s| s.borrow_mut().quit_apps.insert(main_pid));
            Ok(())
        }
        None => Err(format!("La app (PID {main_pid}) ya no está en ejecución.")),
    }
}

// ---------------------------------------------------------------------------
// Installed apps
// ---------------------------------------------------------------------------

fn leftover(path: &str, kind: &str, size: u64) -> AppLeftover {
    AppLeftover { path: expand(path), kind: kind.into(), size_bytes: size }
}

struct MockApp {
    name: &'static str,
    bundle: &'static str,
    version: &'static str,
    size: u64,
    leftovers: Vec<AppLeftover>,
    last_used: Option<u64>,
    uses: Option<u64>,
    added: u64,
    running: bool,
    electron: bool,
    app_store: bool,
    login: bool,
    apple: bool,
}

fn std_leftovers(name: &str, bundle: &str, support: u64, cache: u64) -> Vec<AppLeftover> {
    let mut v = vec![
        leftover(&format!("~/Library/Application Support/{name}"), "Soporte", support),
        leftover(&format!("~/Library/Caches/{bundle}"), "Caché", cache),
        leftover(&format!("~/Library/Preferences/{bundle}.plist"), "Preferencias", 12 * KB),
    ];
    v.retain(|l| l.size_bytes > 0);
    v
}

fn mock_apps() -> Vec<MockApp> {
    let app = |name, bundle, version, size, leftovers, last_used, uses, added| MockApp {
        name,
        bundle,
        version,
        size,
        leftovers,
        last_used,
        uses,
        added,
        running: false,
        electron: false,
        app_store: false,
        login: false,
        apple: false,
    };
    vec![
        MockApp { running: true, ..app("Google Chrome", "com.google.Chrome", "131.0.6778.86", 1_420 * MB, std_leftovers("Google/Chrome", "com.google.Chrome", 2_840 * MB, 1_210 * MB), Some(0), Some(2_904), 610) },
        MockApp { running: true, login: true, ..app("Docker", "com.docker.docker", "4.36.0", 2_140 * MB, vec![
            leftover("~/Library/Containers/com.docker.docker", "Contenedor", 38_400 * MB),
            leftover("~/Library/Group Containers/group.com.docker", "Contenedor", 210 * MB),
            leftover("~/Library/Preferences/com.docker.docker.plist", "Preferencias", 8 * KB),
        ], Some(0), Some(312), 420) },
        MockApp { app_store: true, ..app("Xcode", "com.apple.dt.Xcode", "16.1", 12_600 * MB, vec![
            leftover("~/Library/Developer/Xcode/DerivedData", "Caché", 18_200 * MB),
            leftover("~/Library/Developer/Xcode/iOS DeviceSupport", "Soporte", 9_400 * MB),
            leftover("~/Library/Caches/com.apple.dt.Xcode", "Caché", 640 * MB),
        ], Some(6), Some(188), 700) },
        MockApp { running: true, electron: true, ..app("Claude", "com.anthropic.claudefordesktop", "0.9.3", 512 * MB, std_leftovers("Claude", "com.anthropic.claudefordesktop", 380 * MB, 220 * MB), Some(0), Some(940), 220) },
        MockApp { running: true, electron: true, ..app("Visual Studio Code", "com.microsoft.VSCode", "1.95.3", 590 * MB, std_leftovers("Code", "com.microsoft.VSCode", 1_640 * MB, 420 * MB), Some(0), Some(1_820), 900) },
        MockApp { running: true, electron: true, ..app("Slack", "com.tinyspeck.slackmacgap", "4.41.97", 520 * MB, std_leftovers("Slack", "com.tinyspeck.slackmacgap", 1_090 * MB, 160 * MB), Some(0), Some(2_210), 1_100) },
        MockApp { running: true, electron: true, login: true, ..app("Spotify", "com.spotify.client", "1.2.50", 410 * MB, std_leftovers("Spotify", "com.spotify.client", 140 * MB, 6_800 * MB), Some(3), Some(760), 1_300) },
        MockApp { electron: true, ..app("Notion", "notion.id", "4.2.0", 480 * MB, std_leftovers("Notion", "notion.id", 610 * MB, 95 * MB), Some(9), Some(140), 500) },
        MockApp { electron: true, ..app("Figma", "com.figma.Desktop", "124.6.5", 490 * MB, std_leftovers("Figma", "com.figma.Desktop", 320 * MB, 880 * MB), Some(34), Some(61), 640) },
        app("Webex", "com.cisco.webexmeetingsapp", "44.10", 380 * MB, std_leftovers("Webex", "com.cisco.webexmeetingsapp", 520 * MB, 44 * MB), Some(47), Some(23), 800),
        app("VLC", "org.videolan.vlc", "3.0.21", 140 * MB, std_leftovers("org.videolan.vlc", "org.videolan.vlc", 2 * MB, 18 * MB), Some(63), Some(9), 1_400),
        MockApp { login: true, ..app("Dropbox", "com.getdropbox.dropbox", "210.4", 162 * MB, std_leftovers("Dropbox", "com.getdropbox.dropbox", 210 * MB, 34 * MB), Some(214), Some(4), 520) },
        MockApp { electron: true, login: true, ..app("Microsoft Teams", "com.microsoft.teams2", "24295.605", 1_120 * MB, std_leftovers("Microsoft/Teams", "com.microsoft.teams2", 2_310 * MB, 480 * MB), Some(142), Some(11), 690) },
        app("Adobe Acrobat Reader", "com.adobe.Reader", "24.004", 1_610 * MB, std_leftovers("Adobe/Acrobat", "com.adobe.Reader", 310 * MB, 120 * MB), Some(121), Some(6), 740),
        MockApp { login: true, ..app("Steam", "com.valvesoftware.steam", "4.0", 680 * MB, vec![
            leftover("~/Library/Application Support/Steam", "Soporte", 4_420 * MB),
            leftover("~/Library/Caches/com.valvesoftware.steam", "Caché", 96 * MB),
        ], Some(301), Some(3), 900) },
        MockApp { app_store: true, apple: true, ..app("GarageBand", "com.apple.garageband10", "10.4.11", 1_930 * MB, vec![
            leftover("/Library/Application Support/GarageBand", "Soporte", 2_640 * MB),
        ], None, Some(0), 330) },
        MockApp { app_store: true, apple: true, ..app("iMovie", "com.apple.iMovieApp", "10.4.2", 3_110 * MB, vec![], Some(388), Some(2), 820) },
        MockApp { login: true, ..app("Creative Cloud", "com.adobe.acc.AdobeCreativeCloud", "6.4", 420 * MB, std_leftovers("Adobe/CreativeCloud", "com.adobe.acc.AdobeCreativeCloud", 96 * MB, 12 * MB), None, None, 260) },
    ]
}

fn verdict_for(a: &MockApp) -> (UsageVerdict, String) {
    match (a.last_used, a.uses) {
        (_, None) => (
            UsageVerdict::Unknown,
            "Spotlight no tiene datos de uso de esta app.".into(),
        ),
        (None, _) => (
            UsageVerdict::Unused,
            format!("Nunca la has abierto desde que se instaló {}.", crate::fmt::days_ago(Some(a.added))),
        ),
        (Some(d), _) if d <= 30 => (
            UsageVerdict::Active,
            if a.running { "En ejecución ahora.".into() } else { format!("La usaste {}.", crate::fmt::days_ago(Some(d))) },
        ),
        (Some(d), _) if d <= 90 => (UsageVerdict::Occasional, format!("La abriste por última vez {}.", crate::fmt::days_ago(Some(d)))),
        (Some(d), uses) => (
            UsageVerdict::Unused,
            format!(
                "Sin abrir {}{}.",
                crate::fmt::days_ago(Some(d)),
                match uses {
                    Some(n) if n <= 5 => format!(" · solo {n} usos en total"),
                    _ => String::new(),
                }
            ),
        ),
    }
}

struct MockOrphan {
    bundle: &'static str,
    age: u64,
    leftovers: Vec<AppLeftover>,
}

fn mock_orphans() -> Vec<MockOrphan> {
    vec![
        MockOrphan { bundle: "com.evernote.evernote", age: 640, leftovers: vec![
            leftover("~/Library/Application Support/com.evernote.evernote", "Soporte", 182 * MB),
            leftover("~/Library/Caches/com.evernote.evernote", "Caché", 41 * MB),
            leftover("~/Library/Preferences/com.evernote.evernote.plist", "Preferencias", 6 * KB),
        ] },
        MockOrphan { bundle: "com.parallels.desktop.console", age: 410, leftovers: vec![
            leftover("~/Library/Group Containers/4C6364ACXT.com.parallels.desktop.appstore", "Contenedor", 1_240 * MB),
            leftover("~/Library/Caches/com.parallels.desktop.console", "Caché", 310 * MB),
            leftover("~/Library/Logs/parallels.log", "Registros", 52 * MB),
        ] },
        MockOrphan { bundle: "com.microsoft.edgemac", age: 233, leftovers: vec![
            leftover("~/Library/Application Support/Microsoft Edge", "Soporte", 860 * MB),
            leftover("~/Library/Caches/com.microsoft.edgemac", "Caché", 420 * MB),
        ] },
        MockOrphan { bundle: "com.avast.avast", age: 520, leftovers: vec![
            leftover("~/Library/Application Support/Avast", "Soporte", 96 * MB),
            leftover("~/Library/LaunchAgents/com.avast.home.userinit.plist", "Inicio", 2 * KB),
        ] },
        MockOrphan { bundle: "com.adobe.acc.AdobeCreativeCloud", age: 95, leftovers: vec![
            leftover("~/Library/Application Support/Adobe/OOBE", "Soporte", 288 * MB),
            leftover("~/Library/Caches/com.adobe.acc.AdobeCreativeCloud", "Caché", 74 * MB),
        ] },
    ]
}

pub async fn apps_report() -> AppsReport {
    sleep(2_800).await;
    let deleted = SIM.with(|s| s.borrow().deleted.clone());
    let quit = SIM.with(|s| s.borrow().quit_apps.clone());
    let quit_names: Vec<&str> = MEM_APPS.iter().filter(|a| quit.contains(&a.7)).map(|a| a.0).collect();

    let mut apps: Vec<InstalledApp> = mock_apps()
        .into_iter()
        .map(|a| (format!("/Applications/{}.app", a.name), a))
        .filter(|(path, _)| !deleted.contains(path))
        .map(|(path, mut a)| {
            if quit_names.contains(&a.name) {
                a.running = false;
            }
            let leftovers: Vec<AppLeftover> =
                a.leftovers.iter().filter(|l| !deleted.contains(&l.path)).cloned().collect();
            let (verdict, reason) = verdict_for(&a);
            InstalledApp {
                path,
                name: a.name.into(),
                bundle_id: Some(a.bundle.into()),
                version: Some(a.version.into()),
                size_bytes: a.size,
                leftovers_bytes: leftovers.iter().map(|l| l.size_bytes).sum(),
                leftovers,
                last_used_days_ago: a.last_used,
                use_count: a.uses,
                added_days_ago: Some(a.added),
                is_running: a.running,
                is_electron: a.electron,
                from_app_store: a.app_store,
                launches_at_login: a.login,
                verdict,
                reason,
                removable: !a.apple && !a.running,
            }
        })
        .collect();
    apps.sort_by_key(|a| std::cmp::Reverse(a.size_bytes + a.leftovers_bytes));

    let orphans: Vec<OrphanGroup> = mock_orphans()
        .into_iter()
        .filter_map(|o| {
            let leftovers: Vec<AppLeftover> =
                o.leftovers.into_iter().filter(|l| !deleted.contains(&l.path)).collect();
            (!leftovers.is_empty()).then(|| OrphanGroup {
                bundle_id: o.bundle.into(),
                total_bytes: leftovers.iter().map(|l| l.size_bytes).sum(),
                leftovers,
                modified_days_ago: Some(o.age),
            })
        })
        .collect();

    AppsReport {
        unused_bytes: apps
            .iter()
            .filter(|a| a.verdict == UsageVerdict::Unused)
            .map(|a| a.size_bytes + a.leftovers_bytes)
            .sum(),
        orphans_bytes: orphans.iter().map(|o| o.total_bytes).sum(),
        apps,
        orphans,
    }
}

pub async fn preview_uninstall(app_path: &str) -> Result<CleanupPreview, String> {
    sleep(500).await;
    let deleted = SIM.with(|s| s.borrow().deleted.clone());
    let a = mock_apps()
        .into_iter()
        .find(|a| format!("/Applications/{}.app", a.name) == app_path)
        .ok_or_else(|| "La app ya no está instalada.".to_string())?;
    if a.apple {
        return Err("Las apps de Apple no se pueden desinstalar.".into());
    }
    let mut items = vec![PreviewItem {
        path: app_path.into(),
        size_bytes: a.size,
        is_dir: true,
        category_id: "uninstall".into(),
        modified_days_ago: a.last_used,
    }];
    items.extend(a.leftovers.iter().filter(|l| !deleted.contains(&l.path)).map(|l| PreviewItem {
        path: l.path.clone(),
        size_bytes: l.size_bytes,
        is_dir: !l.path.ends_with(".plist"),
        category_id: "uninstall".into(),
        modified_days_ago: a.last_used,
    }));
    Ok(new_preview(items))
}

pub async fn preview_orphans(bundle_ids: &[String]) -> CleanupPreview {
    sleep(400).await;
    let deleted = SIM.with(|s| s.borrow().deleted.clone());
    let items = mock_orphans()
        .into_iter()
        .filter(|o| bundle_ids.iter().any(|b| b == o.bundle))
        .flat_map(|o| {
            let age = o.age;
            o.leftovers.into_iter().map(move |l| PreviewItem {
                is_dir: !l.path.ends_with(".plist") && !l.path.ends_with(".log"),
                path: l.path,
                size_bytes: l.size_bytes,
                category_id: "orphans".into(),
                modified_days_ago: Some(age),
            })
        })
        .filter(|i| !deleted.contains(&i.path))
        .collect();
    new_preview(items)
}

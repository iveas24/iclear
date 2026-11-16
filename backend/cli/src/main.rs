//! iclear-cli: diagnostics from the terminal.
//!
//!   iclear-cli battery            battery info as JSON
//!   iclear-cli metrics            one full metrics snapshot
//!   iclear-cli fans               fans
//!   iclear-cli temps              temperature sensors
//!   iclear-cli smc dump [PREFIX]  every SMC key (optionally filtered) with type and value
//!   iclear-cli smc read KEY       one SMC key
//!   iclear-cli scan [PATH]        folder usage (default $HOME)
//!   iclear-cli large [PATH] [MB]  biggest files
//!   iclear-cli categories         cleanup categories with sizes (dry-run, deletes nothing)
//!   iclear-cli agents             launch agents/daemons
//!   iclear-cli helper             helper status
//!   iclear-cli cloud              synced cloud drives
//!   iclear-cli cloud stats ID     sync stats of one drive (sizes, % downloaded / uploaded)
//!   iclear-cli cloud ls ID [PATH] one folder with per-item sync state
//!   iclear-cli cloud backups      configured cloud backups

use std::path::PathBuf;

use iclear_core::{battery, cloud, helper_client, launchd, sensors, smc::Smc, storage, system};
use iclear_protocol::format_bytes;

fn print_json<T: serde::Serialize>(v: &T) {
    println!("{}", serde_json::to_string_pretty(v).unwrap());
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a = |i: usize| args.get(i).map(String::as_str);
    match a(0) {
        Some("battery") => print_json(&battery::read()),
        Some("cloud") => {
            let home = storage::home();
            let t = std::time::Instant::now();
            match (a(1), a(2)) {
                (Some("stats"), Some(id)) => match cloud::stats(&home, id, std::time::Duration::from_secs(a(3).and_then(|s| s.parse().ok()).unwrap_or(600))) {
                    Ok(s) => print_json(&s),
                    Err(e) => eprintln!("{e}"),
                },
                (Some("ls"), Some(id)) => match cloud::list(&home, &cloud::Scans::new(), id, a(3)) {
                    Ok(l) => {
                        for i in &l.items {
                            println!(
                                "{:<14} {:>10} ↓{:>5.1}% ↑{:>5.1}%  {}{}",
                                format!("{:?}", i.state),
                                format_bytes(i.size_bytes),
                                i.download_pct,
                                i.upload_pct,
                                i.name,
                                if i.is_dir { "/" } else { "" }
                            );
                        }
                    }
                    Err(e) => eprintln!("{e}"),
                },
                (Some("backups"), _) => print_json(&cloud::BackupService::new(home, cloud::Transfers::new()).list()),
                _ => print_json(&cloud::providers(&home)),
            }
            eprintln!("({} ms)", t.elapsed().as_millis());
        }
        Some("metrics") => {
            let mut c = system::Collector::new();
            std::thread::sleep(std::time::Duration::from_millis(500));
            print_json(&c.metrics());
        }
        Some("fans") => match Smc::open() {
            Ok(smc) => print_json(&sensors::read_fans(&smc)),
            Err(e) => eprintln!("SMC: {e}"),
        },
        Some("temps") => match sensors::Sensors::new() {
            Some(s) => {
                let t = s.temps();
                for x in &t {
                    println!("{:<28} {:<10} {:>6.1} °C", x.label, x.group, x.celsius);
                }
                print_json(&sensors::summarize(&t));
            }
            None => eprintln!("No se pudo abrir el SMC"),
        },
        Some("smc") => {
            let smc = match Smc::open() {
                Ok(s) => s,
                Err(e) => return eprintln!("SMC: {e}"),
            };
            match a(1) {
                Some("read") => {
                    let key = a(2).unwrap_or("FNum");
                    match smc.read(key) {
                        Ok(v) => println!("{key} [{}] size={} bytes={:02x?} value={:?}", v.info.type_str(), v.info.size, v.bytes, v.as_f64()),
                        Err(e) => eprintln!("{e}"),
                    }
                }
                _ => {
                    let prefix = a(2).or(a(1).filter(|s| *s != "dump")).unwrap_or("");
                    let keys = smc.all_keys();
                    println!("# {} llaves", keys.len());
                    for k in keys.iter().filter(|k| k.starts_with(prefix)) {
                        match smc.read(k) {
                            Ok(v) => println!("{k} [{}] {:02x?} {:?}", v.info.type_str(), v.bytes, v.as_f64()),
                            Err(_) => println!("{k} (no legible)"),
                        }
                    }
                }
            }
        }
        Some("scan") => {
            let root = a(1).map(PathBuf::from).unwrap_or_else(storage::home);
            match storage::scan::usage(&root) {
                Ok(r) => {
                    for c in r.children.iter().take(30) {
                        println!("{:>10}  {}", format_bytes(c.size_bytes), c.name);
                    }
                    println!("{:>10}  TOTAL ({} sin acceso)", format_bytes(r.total_bytes), r.skipped_errors);
                }
                Err(e) => eprintln!("{e}"),
            }
        }
        Some("large") => {
            let root = a(1).map(PathBuf::from).unwrap_or_else(storage::home);
            let mb: u64 = a(2).and_then(|s| s.parse().ok()).unwrap_or(500);
            for f in storage::scan::large_files(&root, mb * 1_000_000, 30, &[]) {
                println!("{:>10}  {}", format_bytes(f.size_bytes), f.path);
            }
        }
        Some("categories") => {
            for c in storage::summaries(&storage::home()) {
                println!("{:<26} {:>10}  {:>5} ítems  {:?}", c.name, format_bytes(c.size_bytes), c.item_count, c.risk);
            }
        }
        Some("memory") => {
            let mut intel = iclear_core::memory::MemoryIntel::new();
            // Two passes so CPU usage has a baseline.
            let _ = intel.report();
            std::thread::sleep(std::time::Duration::from_millis(1500));
            let r = intel.report();
            if a(1) == Some("--json") {
                return print_json(&r);
            }
            let b = &r.breakdown;
            let m = iclear_core::memory::fmt_mem;
            println!("Salud {} / 100 · presión {:?} · libre {} %", r.score, b.pressure, b.free_percent);
            println!(
                "Total {} · apps {} · fija {} · comprimida {} (guarda {}) · caché {} · libre {} · swap {}/{}",
                m(b.total), m(b.app), m(b.wired), m(b.compressed), m(b.compressed_original), m(b.cached), m(b.free), m(b.swap_used), m(b.swap_total)
            );
            println!("\n{:<34} {:>9} {:>5} {:>5} {:>6}  tipo", "app", "memoria", "proc", "tabs", "cpu");
            for x in r.apps.iter().take(25) {
                println!("{:<34} {:>9} {:>5} {:>5} {:>5.1}%  {:?}{}", x.name.chars().take(34).collect::<String>(), m(x.footprint_bytes), x.process_count, x.renderer_count, x.cpu, x.kind, if x.is_gui { " · ventana" } else { "" });
            }
            println!("\nRecomendaciones:");
            for i in &r.insights {
                println!("[{:?}] {}\n    {}", i.severity, i.title, i.detail);
            }
        }
        Some("apps") if a(1) == Some("preview") => {
            // Dry run: what an uninstall would delete (passes the same guard, deletes nothing).
            let home = storage::home();
            let store = storage::PreviewStore::new(home.clone());
            let target = a(2).unwrap_or("");
            let items = if target.ends_with(".app") {
                match iclear_core::apps::uninstall_items(&home, std::path::Path::new(target)) {
                    Ok(i) => i,
                    Err(e) => return eprintln!("{e}"),
                }
            } else {
                iclear_core::apps::orphan_items(&home, &[target.to_string()])
            };
            let offered = items.len();
            let preview = store.create_app_removal(items);
            for i in &preview.items {
                println!("{:>10}  {}", format_bytes(i.size_bytes), i.path);
            }
            println!("TOTAL {} · {} de {} pasan la guarda (no se borró nada)", format_bytes(preview.total_bytes), preview.items.len(), offered);
        }
        Some("apps") => {
            let focus = iclear_core::memory::MemoryIntel::new().focus_map().clone();
            let r = iclear_core::apps::scan(&storage::home(), &focus);
            if a(1) == Some("--json") {
                return print_json(&r);
            }
            for x in &r.apps {
                println!(
                    "{:<30} {:>9} +{:>9}  {:<10?} {}{}{}{}",
                    x.name.chars().take(30).collect::<String>(),
                    format_bytes(x.size_bytes),
                    format_bytes(x.leftovers_bytes),
                    x.verdict,
                    x.reason,
                    if x.launches_at_login { " · al iniciar" } else { "" },
                    if x.is_electron { " · electron" } else { "" },
                    if x.removable { "" } else { " · no removible" }
                );
            }
            println!("\nSin uso: {} · {} apps", format_bytes(r.unused_bytes), r.apps.len());
            println!("Restos huérfanos: {}", format_bytes(r.orphans_bytes));
            for o in r.orphans.iter().take(30) {
                println!("  {:<45} {:>9}  {} ítems, {:?} días", o.bundle_id, format_bytes(o.total_bytes), o.leftovers.len(), o.modified_days_ago);
            }
        }
        Some("agents") => {
            for l in launchd::list() {
                println!("{:<14} {} {} {}", l.scope, if l.loaded { "●" } else { "○" }, if l.disabled { "off" } else { "on " }, l.label);
            }
        }
        Some("helper") if a(1) == Some("send") => {
            // e.g. iclear-cli helper send '{"cmd":"set_fan_rpm","fan":0,"rpm":3000}'
            let req: iclear_protocol::HelperRequest = match serde_json::from_str(a(2).unwrap_or("")) {
                Ok(r) => r,
                Err(e) => return eprintln!("JSON inválido: {e}"),
            };
            match helper_client::send(&req) {
                Ok(r) => print_json(&r),
                Err(e) => eprintln!("{e}"),
            }
        }
        Some("helper") => print_json(&helper_client::status()),
        _ => {
            eprintln!("uso: iclear-cli <battery|metrics|fans|temps|smc dump [PREFIJO]|smc read KEY|scan [RUTA]|large [RUTA] [MB]|categories|agents|helper>");
            std::process::exit(2);
        }
    }
}

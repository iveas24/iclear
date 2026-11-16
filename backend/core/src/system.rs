//! CPU, memory, network, disks and processes via `sysinfo`, plus the
//! `Collector` that assembles a full `Metrics` snapshot.

use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use iclear_protocol::{DiskInfo, MachineInfo, Metrics, ProcessInfo, SystemSnapshot};
use sysinfo::{Disks, Networks, Pid, ProcessesToUpdate, Signal, System, Users};

use crate::{battery, sensors};

pub struct Collector {
    sys: System,
    networks: Networks,
    disks: Disks,
    users: Users,
    sensors: Option<sensors::Sensors>,
    last_net: Instant,
}

impl Default for Collector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        Self {
            sys,
            networks: Networks::new_with_refreshed_list(),
            disks: Disks::new_with_refreshed_list(),
            users: Users::new_with_refreshed_list(),
            sensors: sensors::Sensors::new(),
            last_net: Instant::now(),
        }
    }

    pub fn sensors(&self) -> Option<&sensors::Sensors> {
        self.sensors.as_ref()
    }

    pub fn snapshot(&mut self) -> SystemSnapshot {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.networks.refresh(true);
        self.disks.refresh(true);

        let elapsed = self.last_net.elapsed().as_secs_f64().max(0.001);
        self.last_net = Instant::now();
        let (rx, tx) = self
            .networks
            .iter()
            .filter(|(name, _)| !name.starts_with("lo"))
            .fold((0u64, 0u64), |(r, t), (_, n)| (r + n.received(), t + n.transmitted()));

        let load = System::load_average();
        SystemSnapshot {
            cpu_total: self.sys.global_cpu_usage(),
            cpu_per_core: self.sys.cpus().iter().map(|c| c.cpu_usage()).collect(),
            mem_total: self.sys.total_memory(),
            mem_used: self.sys.used_memory(),
            swap_total: self.sys.total_swap(),
            swap_used: self.sys.used_swap(),
            load_avg: [load.one, load.five, load.fifteen],
            uptime_secs: System::uptime(),
            net_rx_per_sec: (rx as f64 / elapsed) as u64,
            net_tx_per_sec: (tx as f64 / elapsed) as u64,
            disks: self
                .disks
                .iter()
                .filter(|d| {
                    let m = d.mount_point().to_string_lossy();
                    m == "/" || m.starts_with("/Volumes/")
                })
                .map(|d| DiskInfo {
                    name: d.name().to_string_lossy().to_string(),
                    mount_point: d.mount_point().to_string_lossy().to_string(),
                    total_bytes: d.total_space(),
                    available_bytes: d.available_space(),
                })
                .collect(),
            process_count: self.sys.processes().len(),
        }
    }

    pub fn metrics(&mut self) -> Metrics {
        let system = self.snapshot();
        let (fans, temps) = match &self.sensors {
            Some(s) => (s.fans(), s.temps()),
            None => (vec![], vec![]),
        };
        Metrics {
            timestamp_ms: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
            system,
            battery: battery::read(),
            temp_summary: sensors::summarize(&temps),
            fans,
            temps,
        }
    }

    pub fn processes(&mut self) -> Vec<ProcessInfo> {
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
        let me = unsafe { libc::getuid() };
        let mut list: Vec<ProcessInfo> = self
            .sys
            .processes()
            .iter()
            .map(|(pid, p)| {
                let uid = p.user_id().map(|u| **u);
                ProcessInfo {
                    pid: pid.as_u32(),
                    name: p.name().to_string_lossy().to_string(),
                    cpu: p.cpu_usage(),
                    memory: p.memory(),
                    user: p.user_id().and_then(|u| self.users.get_user_by_id(u)).map(|u| u.name().to_string()),
                    killable: uid == Some(me) && pid.as_u32() != std::process::id(),
                }
            })
            .collect();
        list.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal));
        list
    }

    /// Sends SIGTERM (or SIGKILL when `force`) to a process owned by the current user.
    pub fn kill(&mut self, pid: u32, force: bool) -> Result<(), String> {
        self.sys.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
        let p = self.sys.process(Pid::from_u32(pid)).ok_or("El proceso ya no existe")?;
        let me = unsafe { libc::getuid() };
        if p.user_id().map(|u| **u) != Some(me) {
            return Err("Solo se pueden terminar procesos de tu usuario".into());
        }
        let signal = if force { Signal::Kill } else { Signal::Term };
        match p.kill_with(signal) {
            Some(true) => Ok(()),
            _ => Err("No se pudo enviar la señal".into()),
        }
    }
}

fn sysctl(name: &str) -> String {
    Command::new("/usr/sbin/sysctl")
        .args(["-n", name])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

pub fn machine_info() -> MachineInfo {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_list(sysinfo::CpuRefreshKind::nothing());
    let os = Command::new("/usr/bin/sw_vers")
        .output()
        .map(|o| {
            let s = String::from_utf8_lossy(&o.stdout).to_string();
            let get = |k: &str| {
                s.lines().find(|l| l.starts_with(k)).and_then(|l| l.split(':').nth(1)).map(|v| v.trim().to_string()).unwrap_or_default()
            };
            format!("macOS {} ({})", get("ProductVersion"), get("BuildVersion"))
        })
        .unwrap_or_default();
    MachineInfo {
        model: sysctl("hw.model"),
        chip: sysctl("machdep.cpu.brand_string"),
        os_version: os,
        hostname: System::host_name().unwrap_or_default(),
        cpu_cores: sys.cpus().len(),
        mem_total: sys.total_memory(),
    }
}

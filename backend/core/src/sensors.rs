//! Fans and temperature sensors read from the SMC.
//!
//! Key names differ between chips, so temperature keys are discovered once by
//! enumerating the SMC and keeping `T???` keys that decode to a plausible
//! temperature.

use iclear_protocol::{FanInfo, FanMode, TempSensor, TempSummary};

use crate::smc::Smc;

pub struct Sensors {
    smc: Smc,
    temp_keys: Vec<(String, String, String)>, // key, label, group
}

impl Sensors {
    pub fn new() -> Option<Self> {
        let smc = Smc::open().ok()?;
        let temp_keys = discover_temp_keys(&smc);
        Some(Self { smc, temp_keys })
    }

    pub fn smc(&self) -> &Smc {
        &self.smc
    }

    pub fn fans(&self) -> Vec<FanInfo> {
        read_fans(&self.smc)
    }

    pub fn temps(&self) -> Vec<TempSensor> {
        self.temp_keys
            .iter()
            .filter_map(|(key, label, group)| {
                let c = self.smc.read_f64(key)? as f32;
                plausible(c).then(|| TempSensor { key: key.clone(), label: label.clone(), group: group.clone(), celsius: c })
            })
            .collect()
    }
}

pub fn read_fans(smc: &Smc) -> Vec<FanInfo> {
    let count = smc.read_f64("FNum").unwrap_or(0.0) as u8;
    (0..count)
        .filter_map(|i| {
            let actual = smc.read_f64(&format!("F{i}Ac"))? as f32;
            // F{i}Md: 1 = manual. 0 = automatic; 3 = automatic with the fan
            // stopped by the system (observed on Apple Silicon / macOS 27).
            let mode = match smc.read_f64(&format!("F{i}Md")) {
                Some(v) if v == 1.0 => FanMode::Manual,
                Some(_) => FanMode::Auto,
                None => FanMode::Unknown,
            };
            Some(FanInfo {
                index: i,
                actual_rpm: actual.max(0.0),
                min_rpm: smc.read_f64(&format!("F{i}Mn")).unwrap_or(0.0) as f32,
                max_rpm: smc.read_f64(&format!("F{i}Mx")).unwrap_or(0.0) as f32,
                target_rpm: smc.read_f64(&format!("F{i}Tg")).map(|v| v as f32),
                mode,
            })
        })
        .collect()
}

fn plausible(c: f32) -> bool {
    c.is_finite() && (5.0..=130.0).contains(&c)
}

fn classify(key: &str) -> Option<(&'static str, &'static str)> {
    let p = &key[..2];
    Some(match p {
        "Tp" => ("CPU", "CPU P-core"),
        "Te" => ("CPU", "CPU E-core"),
        "Tf" => ("CPU", "CPU"),
        "Tg" => ("GPU", "GPU"),
        "Tm" => ("Memoria", "Memoria"),
        "TB" => ("Batería", "Batería"),
        "TH" => ("SSD", "SSD"),
        "TW" => ("Wi-Fi", "Wi-Fi"),
        "Ts" => ("Chasis", "Superficie"),
        "TP" => ("Energía", "Energía"),
        "TV" | "TC" => ("Energía", "Regulador"),
        "Ta" | "TA" => ("Ambiente", "Ambiente"),
        _ => return None,
    })
}

fn discover_temp_keys(smc: &Smc) -> Vec<(String, String, String)> {
    let mut keys: Vec<(String, String, String)> = smc
        .all_keys()
        .into_iter()
        .filter(|k| k.starts_with('T') && k.is_ascii() && k.len() == 4)
        .filter_map(|k| {
            let (group, label) = classify(&k)?;
            let info = smc.key_info(&k).ok()?;
            let t = info.type_str();
            if t != "flt " && t != "sp78" {
                return None;
            }
            let v = smc.read_f64(&k)? as f32;
            plausible(v).then(|| (k.clone(), format!("{label} · {k}"), group.to_string()))
        })
        .collect();
    keys.sort();
    keys
}

pub fn summarize(temps: &[TempSensor]) -> TempSummary {
    let stats = |pred: &dyn Fn(&TempSensor) -> bool| -> (Option<f32>, Option<f32>) {
        let v: Vec<f32> = temps.iter().filter(|t| pred(t)).map(|t| t.celsius).collect();
        if v.is_empty() {
            return (None, None);
        }
        let avg = v.iter().sum::<f32>() / v.len() as f32;
        let max = v.iter().cloned().fold(f32::MIN, f32::max);
        (Some(avg), Some(max))
    };
    let (cpu_avg, cpu_max) = stats(&|t| t.group == "CPU");
    let (gpu_avg, _) = stats(&|t| t.group == "GPU");
    let (battery, _) = stats(&|t| t.group == "Batería");
    let (ssd, _) = stats(&|t| t.group == "SSD");
    TempSummary { cpu_avg, cpu_max, gpu_avg, battery, ssd }
}

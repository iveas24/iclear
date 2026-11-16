//! Battery information from the `AppleSmartBattery` IORegistry entry.
//!
//! We read it through `ioreg -a` (plist output) instead of raw IOKit calls:
//! it is stable across macOS releases and does not need special permissions.

use std::process::Command;

use iclear_protocol::BatteryInfo;
use plist::{Dictionary, Value};

pub fn read() -> Option<BatteryInfo> {
    let out = Command::new("/usr/sbin/ioreg").args(["-rn", "AppleSmartBattery", "-a"]).output().ok()?;
    if !out.status.success() || out.stdout.is_empty() {
        return None;
    }
    parse(&out.stdout)
}

pub fn parse(plist_bytes: &[u8]) -> Option<BatteryInfo> {
    let root: Value = plist::from_bytes(plist_bytes).ok()?;
    let dict = match root {
        Value::Array(items) => items.into_iter().next()?.into_dictionary()?,
        Value::Dictionary(d) => d,
        _ => return None,
    };
    Some(from_dict(&dict))
}

fn int(d: &Dictionary, key: &str) -> Option<i64> {
    match d.get(key)? {
        Value::Integer(i) => i.as_signed().or_else(|| i.as_unsigned().map(|u| u as i64)),
        Value::Boolean(b) => Some(*b as i64),
        _ => None,
    }
}

fn boolean(d: &Dictionary, key: &str) -> bool {
    match d.get(key) {
        Some(Value::Boolean(b)) => *b,
        Some(Value::Integer(i)) => i.as_signed().unwrap_or(0) != 0,
        _ => false,
    }
}

/// IOKit stores signed 64-bit values (amperage) as unsigned integers.
fn signed(v: i64) -> i64 {
    if v > i32::MAX as i64 * 2 {
        v as u64 as i64
    } else {
        v
    }
}

fn from_dict(d: &Dictionary) -> BatteryInfo {
    let nested = d.get("BatteryData").and_then(|v| v.as_dictionary());
    let pick = |key: &str| int(d, key).or_else(|| nested.and_then(|n| int(n, key)));

    let design = pick("DesignCapacity").unwrap_or(0) as u32;
    let max_mah = pick("AppleRawMaxCapacity").or_else(|| pick("NominalChargeCapacity")).unwrap_or(0) as u32;
    let cur_mah = pick("AppleRawCurrentCapacity").unwrap_or(0) as u32;
    let percent = pick("CurrentCapacity").unwrap_or(0).clamp(0, 100) as u8;
    let voltage_mv = pick("Voltage").or_else(|| pick("AppleRawBatteryVoltage")).unwrap_or(0);
    let amperage = signed(pick("InstantAmperage").or_else(|| pick("Amperage")).unwrap_or(0)) as i32;
    let temp_raw = pick("Temperature").or_else(|| pick("VirtualTemperature")).unwrap_or(0);
    let time_remaining = pick("TimeRemaining").filter(|t| *t > 0 && *t < 65535).map(|t| t as u32);

    let adapter = d.get("AdapterDetails").and_then(|v| v.as_dictionary());
    let adapter_watts = adapter.and_then(|a| int(a, "Watts")).map(|w| w as u32);
    let adapter_name = adapter
        .and_then(|a| a.get("Name").or_else(|| a.get("Description")))
        .and_then(|v| v.as_string())
        .map(|s| s.trim().to_string());
    let not_charging_reason = d
        .get("ChargerData")
        .and_then(|v| v.as_dictionary())
        .and_then(|c| int(c, "NotChargingReason"))
        .map(|v| v as u64);

    BatteryInfo {
        percent,
        is_charging: boolean(d, "IsCharging"),
        external_connected: boolean(d, "ExternalConnected"),
        fully_charged: boolean(d, "FullyCharged"),
        cycle_count: pick("CycleCount").unwrap_or(0) as u32,
        design_capacity_mah: design,
        max_capacity_mah: max_mah,
        current_capacity_mah: cur_mah,
        health_percent: if design > 0 { max_mah as f32 / design as f32 * 100.0 } else { 0.0 },
        temperature_c: temp_raw as f32 / 100.0,
        voltage_v: voltage_mv as f32 / 1000.0,
        amperage_ma: amperage,
        power_w: voltage_mv as f32 / 1000.0 * amperage as f32 / 1000.0,
        time_remaining_min: time_remaining,
        adapter_watts: if boolean(d, "ExternalConnected") { adapter_watts } else { None },
        adapter_name: if boolean(d, "ExternalConnected") { adapter_name } else { None },
        not_charging_reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><array><dict>
  <key>CurrentCapacity</key><integer>35</integer>
  <key>IsCharging</key><true/>
  <key>ExternalConnected</key><true/>
  <key>FullyCharged</key><false/>
  <key>CycleCount</key><integer>212</integer>
  <key>DesignCapacity</key><integer>6075</integer>
  <key>AppleRawMaxCapacity</key><integer>5650</integer>
  <key>AppleRawCurrentCapacity</key><integer>1731</integer>
  <key>Temperature</key><integer>3759</integer>
  <key>Voltage</key><integer>11744</integer>
  <key>InstantAmperage</key><integer>18446744073709550616</integer>
  <key>TimeRemaining</key><integer>124</integer>
  <key>AdapterDetails</key><dict><key>Watts</key><integer>70</integer><key>Name</key><string>70W USB-C Power Adapter </string></dict>
</dict></array></plist>"#;

    #[test]
    fn parses_ioreg_plist() {
        let b = parse(SAMPLE.as_bytes()).expect("parse");
        assert_eq!(b.percent, 35);
        assert_eq!(b.cycle_count, 212);
        assert!(b.is_charging && b.external_connected);
        assert!((b.health_percent - 93.0).abs() < 0.1);
        assert!((b.temperature_c - 37.59).abs() < 0.01);
        assert_eq!(b.amperage_ma, -1000);
        assert!(b.power_w < 0.0);
        assert_eq!(b.adapter_watts, Some(70));
        assert_eq!(b.adapter_name.as_deref(), Some("70W USB-C Power Adapter"));
    }
}

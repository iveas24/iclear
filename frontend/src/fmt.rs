//! Display formatting helpers (es-CL: decimal comma, dot thousands separator).

use iclear_protocol::format_bytes;

/// Disk / file sizes (decimal units, like Finder).
pub fn bytes(b: u64) -> String {
    format_bytes(b).replace('.', ",")
}

/// Memory sizes (binary units, like Activity Monitor).
pub fn mem(b: u64) -> String {
    let gib = b as f64 / 1_073_741_824.0;
    if gib >= 1.0 {
        format!("{} GB", dec(gib, if gib >= 100.0 { 0 } else { 1 }))
    } else {
        format!("{} MB", dec(b as f64 / 1_048_576.0, 0))
    }
}

pub fn rate(bps: u64) -> String {
    format!("{}/s", bytes(bps))
}

pub fn dec(v: f64, places: usize) -> String {
    format!("{v:.places$}").replace('.', ",")
}

pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    out
}

pub fn pct(v: f32) -> String {
    format!("{}%", v.round() as i64)
}

/// Progress percentage that never shows 100% (or 0%) until it truly is:
/// 99,96 → "99,9%", 0,02 → "0,1%".
pub fn progress(v: f32) -> String {
    let v = v.clamp(0.0, 100.0);
    if v >= 100.0 || v <= 0.0 {
        format!("{}%", v as i64)
    } else if v >= 99.5 {
        format!("{}%", dec(((v * 10.0).floor() / 10.0).min(99.9) as f64, 1))
    } else if v < 1.0 {
        format!("{}%", dec(((v * 10.0).ceil() / 10.0).max(0.1) as f64, 1))
    } else {
        format!("{}%", v.round() as i64)
    }
}

pub fn temp(c: f32) -> String {
    format!("{} °C", dec(c as f64, 1))
}

pub fn temp_opt(c: Option<f32>) -> String {
    c.map(temp).unwrap_or_else(|| "—".into())
}

pub fn rpm(r: f32) -> String {
    thousands(r.max(0.0).round() as u64)
}

pub fn minutes(m: u32) -> String {
    let (h, m) = (m / 60, m % 60);
    match (h, m) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

pub fn uptime(secs: u64) -> String {
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3600;
    let m = (secs % 3600) / 60;
    if d > 0 {
        format!("{d} d {h} h")
    } else if h > 0 {
        format!("{h} h {m} min")
    } else {
        format!("{m} min")
    }
}

pub fn days_ago(d: Option<u64>) -> String {
    match d {
        None => "—".into(),
        Some(0) => "hoy".into(),
        Some(1) => "hace 1 día".into(),
        Some(d) if d < 60 => format!("hace {d} días"),
        Some(d) if d < 730 => format!("hace {} meses", d / 30),
        Some(d) => format!("hace {} años", d / 365),
    }
}

pub fn file_name(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(path)
        .to_string()
}

/// Replaces `/Users/<name>` with `~`.
pub fn short_path(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("/Users/") {
        match rest.find('/') {
            Some(i) => format!("~{}", &rest[i..]),
            None => "~".into(),
        }
    } else {
        path.to_string()
    }
}

/// Parent directory shown under a file name.
pub fn parent_dir(path: &str) -> String {
    match path.trim_end_matches('/').rfind('/') {
        Some(0) => "/".into(),
        Some(i) => short_path(&path[..i]),
        None => String::new(),
    }
}

pub fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", thousands(n as u64), if n == 1 { one } else { many })
}

/// Memory size without a trailing ",0" ("24 GB", "19,7 GB").
pub fn mem_short(b: u64) -> String {
    mem(b).replace(",0 ", " ")
}

/// Elapsed time from seconds: "hace 2 h", "hace 15 min", "ahora".
pub fn ago_secs(secs: u64) -> String {
    match secs {
        0..=59 => "ahora".into(),
        60..=3599 => format!("hace {} min", secs / 60),
        3600..=86_399 => format!("hace {} h", secs / 3600),
        _ => format!("hace {} d", secs / 86_400),
    }
}

/// Signed growth rate: "+350 MB/h", "−1,2 GB/h".
pub fn per_hour(bytes: i64) -> String {
    let sign = if bytes < 0 { "−" } else { "+" };
    format!("{sign}{}/h", mem(bytes.unsigned_abs()))
}

/// Coarse relative date for app usage; `None` means never opened.
pub fn last_used(days: Option<u64>) -> String {
    match days {
        None => "nunca".into(),
        Some(d) => days_ago(Some(d)),
    }
}

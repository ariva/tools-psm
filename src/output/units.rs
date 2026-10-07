//! Binary units for display, and the size / duration arguments.

use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result, bail};

static UNITS: OnceLock<String> = OnceLock::new();

pub fn set_units(units: &str) -> Result<()> {
    if !["auto", "B", "KiB", "MiB", "GiB"].contains(&units) {
        bail!("display.units must be one of auto, B, KiB, MiB, GiB (got {units:?})");
    }
    let _ = UNITS.set(units.to_string());
    Ok(())
}

/// Binary units, labelled as such, so figures match /proc, `free -h` and `top`.
pub fn human(bytes: i64) -> String {
    const K: f64 = 1024.0;
    let b = bytes.unsigned_abs() as f64;
    let sign = if bytes < 0 { "-" } else { "" };
    let unit = match UNITS.get().map(String::as_str).unwrap_or("auto") {
        "auto" if b >= K * K * K => "GiB",
        "auto" if b >= K * K => "MiB",
        "auto" if b >= K => "KiB",
        "auto" => "B",
        fixed => fixed,
    };
    match unit {
        "GiB" => format!("{sign}{:.2} GiB", b / (K * K * K)),
        "MiB" => format!("{sign}{:.0} MiB", b / (K * K)),
        "KiB" => format!("{sign}{:.0} KiB", b / K),
        _ => format!("{sign}{b:.0} B"),
    }
}

pub fn human_delta(bytes: i64) -> String {
    if bytes > 0 {
        format!("+{}", human(bytes))
    } else {
        human(bytes)
    }
}

/// `500K`, `10M`, `1G` (binary), optionally with `B`/`iB`; a bare number is bytes.
pub fn parse_size(s: &str) -> Result<i64> {
    let t = s.trim();
    let t = t
        .strip_suffix("iB")
        .or_else(|| t.strip_suffix('B'))
        .unwrap_or(t);
    let (num, mult) = match t.chars().last() {
        Some('K' | 'k') => (&t[..t.len() - 1], 1i64 << 10),
        Some('M' | 'm') => (&t[..t.len() - 1], 1 << 20),
        Some('G' | 'g') => (&t[..t.len() - 1], 1 << 30),
        _ => (t, 1),
    };
    let n: f64 = num
        .trim()
        .parse()
        .ok()
        .filter(|n: &f64| n.is_finite() && *n >= 0.0)
        .with_context(|| format!("invalid size {s:?} (examples: 500K, 10M, 1G)"))?;
    Ok((n * mult as f64) as i64)
}

/// `3d 4h`, `2h 13m`, `12m 5s`, `45s`: the two largest units that apply.
pub fn human_duration(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as i64;
    let parts = [
        (total / 86400, "d"),
        (total / 3600 % 24, "h"),
        (total / 60 % 60, "m"),
        (total % 60, "s"),
    ];
    let first = parts.iter().position(|(n, _)| *n > 0).unwrap_or(3);
    let shown: Vec<String> = parts[first..]
        .iter()
        .take(2)
        .filter(|(n, _)| *n > 0)
        .map(|(n, u)| format!("{n}{u}"))
        .collect();
    if shown.is_empty() {
        "0s".into()
    } else {
        shown.join(" ")
    }
}

/// `500ms`, `10s`, `5m`, `2h`, `180d`; a bare number is seconds.
pub fn parse_duration(s: &str) -> Result<Duration> {
    let t = s.trim();
    let (num, secs) = if let Some(n) = t.strip_suffix("ms") {
        (n, 0.001)
    } else if let Some(n) = t.strip_suffix('s') {
        (n, 1.0)
    } else if let Some(n) = t.strip_suffix('m') {
        (n, 60.0)
    } else if let Some(n) = t.strip_suffix('h') {
        (n, 3600.0)
    } else if let Some(n) = t.strip_suffix('d') {
        (n, 86400.0)
    } else {
        (t, 1.0)
    };
    let n: f64 = num
        .trim()
        .parse()
        .ok()
        .filter(|n: &f64| n.is_finite() && *n >= 0.0)
        .with_context(|| format!("invalid duration {s:?} (examples: 500ms, 10s, 5m, 180d)"))?;
    Ok(Duration::from_secs_f64(n * secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_durations() {
        assert_eq!(parse_size("10M").unwrap(), 10 << 20);
        assert_eq!(parse_size("1GiB").unwrap(), 1 << 30);
        assert_eq!(parse_size("500K").unwrap(), 500 << 10);
        assert_eq!(parse_size("42").unwrap(), 42);
        assert!(parse_size("lots").is_err());
        assert!(parse_size("-1M").is_err());
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(
            parse_duration("180d").unwrap(),
            Duration::from_secs(180 * 86400)
        );
        assert_eq!(parse_duration("0").unwrap(), Duration::ZERO);
        assert_eq!(human_duration(45.0), "45s");
        assert_eq!(human_duration(725.0), "12m 5s");
        assert_eq!(human_duration(2.0 * 3600.0 + 13.0 * 60.0 + 7.0), "2h 13m");
        assert_eq!(human_duration(3.0 * 86400.0 + 4.0 * 3600.0), "3d 4h");
        assert_eq!(human_duration(3600.0), "1h");
        assert_eq!(human_duration(60.0), "1m");
        assert_eq!(human_duration(0.0), "0s");
        assert!(parse_duration("soon").is_err());
    }

    #[test]
    fn binary_units() {
        assert_eq!(human(812 << 20), "812 MiB");
        assert_eq!(human(3 << 30), "3.00 GiB");
        assert_eq!(human(512), "512 B");
        assert_eq!(human_delta(-(468 << 20)), "-468 MiB");
        assert_eq!(human_delta(72 << 20), "+72 MiB");
    }
}

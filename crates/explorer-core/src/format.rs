//! Number and date formatting in Explorer's style.

use std::time::SystemTime;

use chrono::{DateTime, Local};

/// Details-view "Size" column: always whole KB, rounded up, with separators
/// ("1 KB", "12,345 KB"); empty files show "0 KB".
pub fn size_kb(bytes: u64) -> String {
    let kb = bytes.div_ceil(1024);
    format!("{} KB", thousands(kb))
}

/// Human size for the status bar and properties ("1.50 MB", "742 bytes").
/// Explorer keeps three significant digits.
pub fn size_human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
    if bytes < 1024 {
        return format!("{bytes} bytes");
    }
    let mut v = bytes as f64 / 1024.0;
    let mut u = 0;
    while v >= 1000.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    let s = if v >= 100.0 {
        format!("{:.0}", v.floor())
    } else if v >= 10.0 {
        format!("{:.1}", (v * 10.0).floor() / 10.0)
    } else {
        format!("{:.2}", (v * 100.0).floor() / 100.0)
    };
    format!("{s} {}", UNITS[u])
}

/// Properties dialog style: "1.50 MB (1,572,864 bytes)".
pub fn size_with_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{} bytes", thousands(bytes));
    }
    format!("{} ({} bytes)", size_human(bytes), thousands(bytes))
}

pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "11/3/2026 2:05 PM" — Explorer's short date + short time.
pub fn datetime(t: Option<SystemTime>) -> String {
    let Some(t) = t else { return String::new() };
    let dt: DateTime<Local> = t.into();
    dt.format("%-m/%-d/%Y %-I:%M %p").to_string()
}

/// Long form used in the Properties dialog.
pub fn datetime_long(t: Option<SystemTime>) -> String {
    let Some(t) = t else { return "Unknown".into() };
    let dt: DateTime<Local> = t.into();
    dt.format("%A, %B %-d, %Y, %-I:%M:%S %p").to_string()
}

pub fn items(n: usize) -> String {
    if n == 1 { "1 item".into() } else { format!("{} items", thousands(n as u64)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kb_column() {
        assert_eq!(size_kb(0), "0 KB");
        assert_eq!(size_kb(1), "1 KB");
        assert_eq!(size_kb(1024), "1 KB");
        assert_eq!(size_kb(1025), "2 KB");
        assert_eq!(size_kb(12_641_280), "12,345 KB");
    }

    #[test]
    fn human() {
        assert_eq!(size_human(742), "742 bytes");
        assert_eq!(size_human(1536), "1.50 KB");
        assert_eq!(size_human(1_572_864), "1.50 MB");
        assert_eq!(size_human(15 * 1024 * 1024), "15.0 MB");
        assert_eq!(size_human(512 * 1024 * 1024), "512 MB");
        assert_eq!(size_human(1000 * 1024), "0.97 MB");
        assert_eq!(size_with_bytes(1_572_864), "1.50 MB (1,572,864 bytes)");
    }

    #[test]
    fn separators() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(1234567), "1,234,567");
        assert_eq!(items(1), "1 item");
        assert_eq!(items(1200), "1,200 items");
    }
}

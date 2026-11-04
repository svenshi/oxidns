// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared helpers for parsing simple runtime values and reading wall-clock or
//! filesystem metadata.

use std::path::Path;
use std::time::Duration;

#[cfg(any(target_os = "linux", test))]
use jiff::fmt::temporal::DateTimeParser;
use jiff::fmt::temporal::DateTimePrinter;
use jiff::tz::TimeZone;
use serde::{Deserialize, Deserializer, de};
use tokio::fs;

use crate::infra::clock::AppClock;

const KIB: u64 = 1024;
const MIB: u64 = KIB * 1024;
const GIB: u64 = MIB * 1024;
const TIB: u64 = GIB * 1024;

/// Read the current wall-clock time since the Unix epoch.
#[inline]
pub fn unix_timestamp() -> u64 {
    AppClock::now_timestamp()
}

/// Resolve the system timezone, including OpenWrt's POSIX `/etc/TZ` file.
///
/// An explicit `TZ` environment variable and the platform's native timezone
/// detection take precedence. Call this during initialization, not per query.
pub fn try_system_timezone() -> Result<TimeZone, jiff::Error> {
    let detected = TimeZone::try_system();
    #[cfg(target_os = "linux")]
    {
        resolve_system_timezone(detected, std::env::var_os("TZ").is_some(), || {
            std::fs::read_to_string("/etc/TZ")
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        detected
    }
}

#[cfg(any(target_os = "linux", test))]
fn resolve_system_timezone(
    detected: Result<TimeZone, jiff::Error>,
    has_tz_override: bool,
    read_fallback: impl FnOnce() -> std::io::Result<String>,
) -> Result<TimeZone, jiff::Error> {
    detected.or_else(|error| {
        if has_tz_override {
            return Err(error);
        }
        read_fallback()
            .ok()
            .and_then(|raw| DateTimeParser::new().parse_time_zone(raw.trim()).ok())
            .ok_or(error)
    })
}

/// Resolve the current system timezone as an IANA name or POSIX rule.
///
/// Returns `UTC` when the timezone cannot be detected or represented by name.
pub fn system_timezone_name() -> String {
    try_system_timezone()
        .ok()
        .and_then(|timezone| timezone_name(&timezone).ok())
        .unwrap_or_else(|| "UTC".to_string())
}

fn timezone_name(timezone: &TimeZone) -> Result<String, jiff::Error> {
    let mut name = String::new();
    DateTimePrinter::new().print_time_zone(timezone, &mut name)?;
    Ok(name)
}

/// Read file length if the path exists.
///
/// Returns `Ok(None)` when the file does not exist.
pub async fn file_len_if_exists(path: impl AsRef<Path>) -> std::io::Result<Option<u64>> {
    match fs::metadata(path).await {
        Ok(metadata) => Ok(Some(metadata.len())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}
/// Parse an integer duration string with an optional unit suffix.
///
/// Supported units:
/// - `ms`
/// - `s`
/// - `m`
/// - `h`
/// - `d`
///
/// If no unit is provided, seconds are used by default.
pub fn parse_simple_duration(raw: &str) -> Result<Duration, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("duration cannot be empty".to_string());
    }

    let (value, unit) = match raw.find(|c: char| !c.is_ascii_digit()) {
        None => {
            let value = raw
                .parse::<u64>()
                .map_err(|e| format!("invalid duration '{}': {}", raw, e))?;

            (value, "s".to_string())
        }
        Some(unit_start) => {
            let value = raw[..unit_start]
                .trim()
                .parse::<u64>()
                .map_err(|e| format!("invalid duration '{}': {}", raw, e))?;

            let unit = raw[unit_start..].trim().to_ascii_lowercase();

            (value, unit)
        }
    };

    let duration = match unit.as_str() {
        "ms" => Duration::from_millis(value),
        "s" => Duration::from_secs(value),
        "m" => Duration::from_secs(value.saturating_mul(60)),
        "h" => Duration::from_secs(value.saturating_mul(60 * 60)),
        "d" => Duration::from_secs(value.saturating_mul(60 * 60 * 24)),
        _ => return Err(format!("unsupported duration unit '{}' in '{}'", unit, raw)),
    };

    Ok(duration)
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum DurationValue {
    String(String),
    U64(u64),
}

/// Deserialize optional duration from either a number or a string.
///
/// Supported examples:
/// - `null`
/// - `3`
/// - `"3"`
/// - `"3s"`
/// - `"500ms"`
///
/// Bare numbers are interpreted as seconds.
pub fn deserialize_duration_option<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<DurationValue>::deserialize(deserializer)?;
    value
        .map(|value| match value {
            DurationValue::U64(seconds) => Ok(Duration::from_secs(seconds)),
            DurationValue::String(raw) => parse_simple_duration(&raw).map_err(de::Error::custom),
        })
        .transpose()
}

/// Parse a file-size string such as `1k`, `2m`, or `3g`.
///
/// Supported suffixes use binary multiples:
/// - `k` / `kb`
/// - `m` / `mb`
/// - `g` / `gb`
/// - `t` / `tb`
///
/// A plain integer is interpreted as raw bytes.
pub fn parse_byte_size(raw: &str) -> Result<u64, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("byte size cannot be empty".to_string());
    }

    if raw.chars().all(|c| c.is_ascii_digit()) {
        return raw
            .parse::<u64>()
            .map_err(|e| format!("invalid byte size '{}': {}", raw, e));
    }

    let unit_start = raw
        .find(|c: char| !c.is_ascii_digit())
        .ok_or_else(|| "byte size must include a valid integer prefix".to_string())?;
    let value = raw[..unit_start]
        .trim()
        .parse::<u64>()
        .map_err(|e| format!("invalid byte size '{}': {}", raw, e))?;
    let unit = raw[unit_start..].trim().to_ascii_lowercase();

    let multiplier = match unit.as_str() {
        "k" | "kb" => KIB,
        "m" | "mb" => MIB,
        "g" | "gb" => GIB,
        "t" | "tb" => TIB,
        _ => {
            return Err(format!(
                "unsupported byte size unit '{}' in '{}'",
                unit, raw
            ));
        }
    };

    value
        .checked_mul(multiplier)
        .ok_or_else(|| format!("byte size '{}' is too large", raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_timezone_name_is_non_empty() {
        assert!(!system_timezone_name().trim().is_empty());
    }

    fn missing_system_timezone() -> Result<TimeZone, jiff::Error> {
        TimeZone::get("Invalid/SystemTimezone")
    }

    #[test]
    fn test_system_timezone_preserves_native_detection_and_tz_override() {
        let timezone = resolve_system_timezone(Ok(TimeZone::UTC), false, || {
            panic!("native detection must take precedence over /etc/TZ")
        })
        .unwrap();
        assert_eq!(timezone, TimeZone::UTC);

        assert!(
            resolve_system_timezone(missing_system_timezone(), true, || {
                panic!("an invalid explicit TZ must not use /etc/TZ")
            })
            .is_err()
        );
    }

    #[test]
    fn test_system_timezone_reads_openwrt_posix_rules() {
        for (raw, winter_offset, summer_offset) in [
            (" CST-8\n", 8, 8),
            ("UTC\n", 0, 0),
            ("EST5EDT,M3.2.0,M11.1.0\n", -5, -4),
        ] {
            let timezone =
                resolve_system_timezone(missing_system_timezone(), false, || Ok(raw.to_string()))
                    .unwrap();
            for (timestamp, hours) in [
                ("2026-01-15T12:00:00Z", winter_offset),
                ("2026-07-15T12:00:00Z", summer_offset),
            ] {
                assert_eq!(
                    timezone.to_offset(timestamp.parse().unwrap()).seconds(),
                    hours * 3600,
                    "{raw:?} at {timestamp}"
                );
            }
            let name = timezone_name(&timezone).unwrap();
            let reparsed = DateTimeParser::new().parse_time_zone(&name).unwrap();
            assert_eq!(timezone, reparsed);
            #[cfg(feature = "_task-cron")]
            crate::infra::task::validate_cron("0 9 * * *", &name).unwrap();
        }
    }

    #[test]
    fn test_system_timezone_rejects_missing_or_invalid_fallback() {
        for raw in ["", " \n", "invalid timezone", "CST-99"] {
            assert!(
                resolve_system_timezone(missing_system_timezone(), false, || Ok(raw.to_string()))
                    .is_err(),
                "{raw:?}"
            );
        }
        assert!(
            resolve_system_timezone(missing_system_timezone(), false, || {
                Err(std::io::ErrorKind::NotFound.into())
            })
            .is_err()
        );
    }

    #[test]
    fn test_parse_simple_duration() {
        assert_eq!(
            parse_simple_duration("250ms").unwrap(),
            Duration::from_millis(250)
        );
        assert_eq!(parse_simple_duration("2s").unwrap(), Duration::from_secs(2));
        assert_eq!(
            parse_simple_duration("3m").unwrap(),
            Duration::from_secs(180)
        );
        assert_eq!(
            parse_simple_duration("4h").unwrap(),
            Duration::from_secs(14_400)
        );
        assert_eq!(
            parse_simple_duration("1d").unwrap(),
            Duration::from_secs(86_400)
        );
        assert_eq!(
            parse_simple_duration("10").unwrap(),
            Duration::from_secs(10)
        );
        assert!(parse_simple_duration("10w").is_err());
        assert!(parse_simple_duration("fuck").is_err());
    }

    #[test]
    fn test_parse_byte_size() {
        assert_eq!(parse_byte_size("128").unwrap(), 128);
        assert_eq!(parse_byte_size("1k").unwrap(), KIB);
        assert_eq!(parse_byte_size("2M").unwrap(), 2 * MIB);
        assert_eq!(parse_byte_size("3gb").unwrap(), 3 * GIB);
        assert!(parse_byte_size("5p").is_err());
        assert!(parse_byte_size("fuck").is_err());
    }
}

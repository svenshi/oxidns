// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `time` matcher plugin.
//!
//! Matches the wall-clock time at which the matcher is evaluated against one
//! or more recurring calendar windows. Each window can constrain a daily
//! `HH:MM` interval, weekdays, and days of the month. Windows are ORed; the
//! conditions inside one window are ANDed.
//!
//! The configured IANA timezone is resolved once during plugin construction.
//! The request path reads the real wall clock, converts it once into that
//! timezone, and compares compact minute and bit-mask representations without
//! allocating or locking.

use async_trait::async_trait;
use jiff::civil::{Date, Weekday};
use jiff::tz::TimeZone;
use jiff::{Timestamp, Zoned};
use serde::Deserialize;
use serde_yaml_ng::Value;
use tracing::info;

use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::infra::error::{DnsError, Result as DnsResult};
use crate::plugin::matcher::Matcher;
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::plugin_factory;

const MAX_PERIODS: usize = 64;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TimeConfig {
    timezone: Option<String>,
    periods: Vec<PeriodConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PeriodConfig {
    start: Option<String>,
    end: Option<String>,
    // Keep raw YAML values until validation so each invalid list element can
    // report its exact configuration path. Weekdays accept the existing
    // `mon` through `sun` names and ISO weekday numbers (`1` through `7`).
    weekdays: Option<Vec<Value>>,
    monthdays: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Copy)]
struct CompiledPeriod {
    start_minute: Option<u16>,
    end_minute: Option<u16>,
    weekday_mask: u8,
    monthday_mask: u32,
}

#[derive(Debug, Clone, Copy)]
struct CalendarDay {
    weekday_bit: u8,
    monthday_bit: u32,
}

#[derive(Debug)]
struct TimeMatcher {
    tag: String,
    timezone: TimeZone,
    timezone_label: String,
    timezone_source: &'static str,
    periods: Vec<CompiledPeriod>,
}

#[derive(Debug, Clone)]
#[plugin_factory("time")]
pub struct TimeFactory;

impl PluginFactory for TimeFactory {
    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> DnsResult<UninitializedPlugin> {
        let config = parse_config(&plugin_config.tag, plugin_config.args.clone())?;
        build_time_matcher(plugin_config.tag.clone(), config)
    }

    fn quick_setup(&self, tag: &str, param: Option<String>) -> DnsResult<UninitializedPlugin> {
        let raw = param
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| DnsError::plugin("time quick setup requires one HH:MM-HH:MM window"))?;
        if raw.split_ascii_whitespace().count() != 1 {
            return Err(DnsError::plugin(
                "time quick setup accepts exactly one HH:MM-HH:MM window",
            ));
        }
        let (start, end) = raw
            .split_once('-')
            .ok_or_else(|| DnsError::plugin("time quick setup must use the HH:MM-HH:MM format"))?;
        let config = TimeConfig {
            timezone: None,
            periods: vec![PeriodConfig {
                start: Some(start.to_string()),
                end: Some(end.to_string()),
                weekdays: None,
                monthdays: None,
            }],
        };
        build_time_matcher(tag.to_string(), config)
    }
}

fn parse_config(tag: &str, args: Option<Value>) -> DnsResult<TimeConfig> {
    let args =
        args.ok_or_else(|| DnsError::plugin(format!("time matcher '{tag}' requires args")))?;
    serde_yaml_ng::from_value(args).map_err(|err| {
        DnsError::plugin(format!(
            "time matcher '{tag}': invalid args configuration: {err}"
        ))
    })
}

fn build_time_matcher(tag: String, config: TimeConfig) -> DnsResult<UninitializedPlugin> {
    let periods = compile_periods(&tag, config.periods)?;
    let (timezone, timezone_label, timezone_source) = resolve_timezone(&tag, config.timezone)?;
    Ok(UninitializedPlugin::Matcher(Box::new(TimeMatcher {
        tag,
        timezone,
        timezone_label,
        timezone_source,
        periods,
    })))
}

fn resolve_timezone(
    tag: &str,
    configured: Option<String>,
) -> DnsResult<(TimeZone, String, &'static str)> {
    match configured {
        Some(value) => {
            let value = value.trim();
            if value.is_empty() {
                return Err(DnsError::plugin(format!(
                    "time matcher '{tag}': timezone cannot be empty"
                )));
            }
            let timezone = TimeZone::get(value).map_err(|err| {
                DnsError::plugin(format!(
                    "time matcher '{tag}': invalid timezone '{value}': {err}"
                ))
            })?;
            Ok((timezone, value.to_string(), "configured"))
        }
        None => {
            let timezone = crate::infra::system::try_system_timezone().map_err(|err| {
                DnsError::plugin(format!(
                    "time matcher '{tag}': failed to resolve system timezone: {err}; configure args.timezone explicitly"
                ))
            })?;
            let timezone_label = timezone
                .iana_name()
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| "system-local".to_string());
            Ok((timezone, timezone_label, "system"))
        }
    }
}

fn compile_periods(tag: &str, periods: Vec<PeriodConfig>) -> DnsResult<Vec<CompiledPeriod>> {
    if periods.is_empty() {
        return Err(DnsError::plugin(format!(
            "time matcher '{tag}': periods must contain at least one item"
        )));
    }
    if periods.len() > MAX_PERIODS {
        return Err(DnsError::plugin(format!(
            "time matcher '{tag}': periods supports at most {MAX_PERIODS} items"
        )));
    }

    periods
        .into_iter()
        .enumerate()
        .map(|(idx, period)| compile_period(tag, idx, period))
        .collect()
}

fn compile_period(tag: &str, index: usize, period: PeriodConfig) -> DnsResult<CompiledPeriod> {
    let path = format!("periods[{index}]");
    let (start_minute, end_minute) = match (period.start, period.end) {
        (Some(start), Some(end)) => {
            let start_minute = parse_minute(tag, &format!("{path}.start"), &start)?;
            let end_minute = parse_minute(tag, &format!("{path}.end"), &end)?;
            if start_minute == end_minute {
                return Err(DnsError::plugin(format!(
                    "time matcher '{tag}': {path}.start and {path}.end must differ"
                )));
            }
            (Some(start_minute), Some(end_minute))
        }
        (None, None) => (None, None),
        _ => {
            return Err(DnsError::plugin(format!(
                "time matcher '{tag}': {path}.start and {path}.end must be configured together"
            )));
        }
    };
    let weekday_mask = compile_weekdays(tag, &path, period.weekdays)?;
    let monthday_mask = compile_monthdays(tag, &path, period.monthdays)?;

    if start_minute.is_none() && weekday_mask == 0 && monthday_mask == 0 {
        return Err(DnsError::plugin(format!(
            "time matcher '{tag}': {path} must define a time window, weekdays, or monthdays"
        )));
    }

    Ok(CompiledPeriod {
        start_minute,
        end_minute,
        weekday_mask,
        monthday_mask,
    })
}

fn parse_minute(tag: &str, path: &str, raw: &str) -> DnsResult<u16> {
    let raw = raw.trim();
    let Some((hour, minute)) = raw.split_once(':') else {
        return Err(invalid_time_error(tag, path, raw));
    };
    if minute.contains(':') || hour.len() != 2 || minute.len() != 2 {
        return Err(invalid_time_error(tag, path, raw));
    }
    let hour = hour
        .parse::<u16>()
        .map_err(|_| invalid_time_error(tag, path, raw))?;
    let minute = minute
        .parse::<u16>()
        .map_err(|_| invalid_time_error(tag, path, raw))?;
    if hour >= 24 || minute >= 60 {
        return Err(invalid_time_error(tag, path, raw));
    }
    Ok(hour * 60 + minute)
}

fn invalid_time_error(tag: &str, path: &str, raw: &str) -> DnsError {
    DnsError::plugin(format!(
        "time matcher '{tag}': {path} must use HH:MM in the range 00:00-23:59, got '{raw}'"
    ))
}

fn compile_weekdays(tag: &str, path: &str, weekdays: Option<Vec<Value>>) -> DnsResult<u8> {
    let Some(weekdays) = weekdays else {
        return Ok(0);
    };
    if weekdays.is_empty() {
        return Err(DnsError::plugin(format!(
            "time matcher '{tag}': {path}.weekdays cannot be empty"
        )));
    }

    let mut mask = 0;
    for (idx, raw) in weekdays.into_iter().enumerate() {
        let value_path = format!("{path}.weekdays[{idx}]");
        let weekday = match raw {
            Value::String(raw) => match raw.trim().to_ascii_lowercase().as_str() {
                "mon" => Weekday::Monday,
                "tue" => Weekday::Tuesday,
                "wed" => Weekday::Wednesday,
                "thu" => Weekday::Thursday,
                "fri" => Weekday::Friday,
                "sat" => Weekday::Saturday,
                "sun" => Weekday::Sunday,
                _ => {
                    return Err(DnsError::plugin(format!(
                        "time matcher '{tag}': {value_path} must be mon, tue, wed, thu, fri, sat, sun, or an ISO weekday number from 1 to 7, got '{raw}'"
                    )));
                }
            },
            Value::Number(number) => {
                let day = number.as_i64().ok_or_else(|| {
                    DnsError::plugin(format!(
                        "time matcher '{tag}': {value_path} must be an ISO weekday number between 1 and 7, got {number}"
                    ))
                })?;
                match day {
                    1 => Weekday::Monday,
                    2 => Weekday::Tuesday,
                    3 => Weekday::Wednesday,
                    4 => Weekday::Thursday,
                    5 => Weekday::Friday,
                    6 => Weekday::Saturday,
                    7 => Weekday::Sunday,
                    _ => {
                        return Err(DnsError::plugin(format!(
                            "time matcher '{tag}': {value_path} must be an ISO weekday number between 1 and 7, got {day}"
                        )));
                    }
                }
            }
            other => {
                return Err(DnsError::plugin(format!(
                    "time matcher '{tag}': {value_path} must be mon, tue, wed, thu, fri, sat, sun, or an ISO weekday number from 1 to 7, got {other:?}"
                )));
            }
        };
        mask |= weekday_bit(weekday);
    }
    Ok(mask)
}

fn compile_monthdays(tag: &str, path: &str, monthdays: Option<Vec<Value>>) -> DnsResult<u32> {
    let Some(monthdays) = monthdays else {
        return Ok(0);
    };
    if monthdays.is_empty() {
        return Err(DnsError::plugin(format!(
            "time matcher '{tag}': {path}.monthdays cannot be empty"
        )));
    }

    let mut mask = 0;
    for (idx, value) in monthdays.into_iter().enumerate() {
        let value_path = format!("{path}.monthdays[{idx}]");
        let day = match value {
            Value::Number(number) => number.as_i64().ok_or_else(|| {
                DnsError::plugin(format!(
                    "time matcher '{tag}': {value_path} must be an integer between 1 and 31, got {number}"
                ))
            })?,
            other => {
                return Err(DnsError::plugin(format!(
                    "time matcher '{tag}': {value_path} must be an integer between 1 and 31, got {other:?}"
                )));
            }
        };
        if !(1..=31).contains(&day) {
            return Err(DnsError::plugin(format!(
                "time matcher '{tag}': {value_path} must be an integer between 1 and 31, got {day}"
            )));
        }
        mask |= monthday_bit(day as u8);
    }
    Ok(mask)
}

impl CompiledPeriod {
    #[inline]
    fn matches(&self, current_minute: u16, today: CalendarDay, yesterday: CalendarDay) -> bool {
        match (self.start_minute, self.end_minute) {
            (None, None) => self.matches_day(today),
            (Some(start), Some(end)) if start < end => {
                current_minute >= start && current_minute < end && self.matches_day(today)
            }
            (Some(start), Some(_)) if current_minute >= start => self.matches_day(today),
            (Some(_), Some(end)) if current_minute < end => self.matches_day(yesterday),
            (Some(_), Some(_)) => false,
            _ => unreachable!("compiled periods always have both time boundaries or neither"),
        }
    }

    #[inline]
    fn matches_day(&self, day: CalendarDay) -> bool {
        (self.weekday_mask == 0 || self.weekday_mask & day.weekday_bit != 0)
            && (self.monthday_mask == 0 || self.monthday_mask & day.monthday_bit != 0)
    }
}

impl TimeMatcher {
    #[inline]
    fn matches_zoned(&self, now: &Zoned) -> bool {
        let current_minute = minute_of_day(now);
        let today = calendar_day(now.date());
        let yesterday = now.date().yesterday().map(calendar_day).unwrap_or(today);
        self.periods
            .iter()
            .any(|period| period.matches(current_minute, today, yesterday))
    }

    #[cfg(test)]
    fn matches_timestamp(&self, timestamp: Timestamp) -> bool {
        self.matches_zoned(&timestamp.to_zoned(self.timezone.clone()))
    }
}

#[inline]
fn minute_of_day(now: &Zoned) -> u16 {
    let time = now.time();
    time.hour() as u16 * 60 + time.minute() as u16
}

#[inline]
fn calendar_day(date: Date) -> CalendarDay {
    CalendarDay {
        weekday_bit: weekday_bit(date.weekday()),
        monthday_bit: monthday_bit(date.day() as u8),
    }
}

#[inline]
fn weekday_bit(weekday: Weekday) -> u8 {
    1 << (weekday as u8 - 1)
}

#[inline]
fn monthday_bit(day: u8) -> u32 {
    1 << (day - 1)
}

#[async_trait]
impl Plugin for TimeMatcher {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> DnsResult<()> {
        info!(
            plugin_tag = %self.tag,
            timezone = %self.timezone_label,
            timezone_source = self.timezone_source,
            periods = self.periods.len(),
            "time matcher initialized"
        );
        Ok(())
    }

    async fn destroy(&self) -> DnsResult<()> {
        Ok(())
    }
}

impl Matcher for TimeMatcher {
    #[hotpath::measure]
    fn is_match(&self, _context: &mut DnsContext) -> bool {
        self.matches_zoned(&Timestamp::now().to_zoned(self.timezone.clone()))
    }
}

#[cfg(test)]
mod tests {
    use serde_yaml_ng::Number;

    use super::*;

    fn monthdays(days: &[u8]) -> Vec<Value> {
        days.iter()
            .map(|&day| Value::Number(Number::from(day as u64)))
            .collect()
    }

    fn weekdays(days: &[&str]) -> Vec<Value> {
        days.iter()
            .map(|day| Value::String((*day).to_string()))
            .collect()
    }

    fn weekday_numbers(days: &[u8]) -> Vec<Value> {
        days.iter()
            .map(|&day| Value::Number(Number::from(day as u64)))
            .collect()
    }

    fn config(periods: Vec<PeriodConfig>) -> TimeConfig {
        TimeConfig {
            timezone: Some("UTC".to_string()),
            periods,
        }
    }

    fn period(start: Option<&str>, end: Option<&str>) -> PeriodConfig {
        PeriodConfig {
            start: start.map(str::to_string),
            end: end.map(str::to_string),
            weekdays: None,
            monthdays: None,
        }
    }

    fn matcher(config: TimeConfig) -> TimeMatcher {
        let tag = "time_test".to_string();
        let (timezone, timezone_label, timezone_source) =
            resolve_timezone(&tag, config.timezone).expect("valid timezone");
        TimeMatcher {
            periods: compile_periods(&tag, config.periods).expect("valid periods"),
            tag,
            timezone,
            timezone_label,
            timezone_source,
        }
    }

    fn timestamp(ms: i64) -> Timestamp {
        Timestamp::from_millisecond(ms).expect("valid timestamp")
    }

    fn parsed_timestamp(raw: &str) -> Timestamp {
        raw.parse().expect("valid RFC 3339 timestamp")
    }

    #[test]
    fn test_parse_minute_validation() {
        assert_eq!(
            parse_minute("time", "periods[0].start", "09:30").unwrap(),
            570
        );
        for value in ["9:30", "09:3", "24:00", "12:60", "09:30:00", "nope"] {
            assert!(parse_minute("time", "periods[0].start", value).is_err());
        }
    }

    #[test]
    fn test_compile_period_validation() {
        assert!(compile_periods("time", vec![]).is_err());
        assert!(compile_period("time", 0, period(Some("09:00"), None)).is_err());
        assert!(compile_period("time", 0, period(Some("09:00"), Some("09:00"))).is_err());
        assert!(compile_period("time", 0, period(None, None)).is_err());
        assert!(
            compile_period(
                "time",
                0,
                PeriodConfig {
                    start: None,
                    end: None,
                    weekdays: Some(vec![]),
                    monthdays: None,
                }
            )
            .is_err()
        );
        assert!(
            compile_period(
                "time",
                0,
                PeriodConfig {
                    start: None,
                    end: None,
                    weekdays: None,
                    monthdays: Some(monthdays(&[0])),
                }
            )
            .is_err()
        );

        for raw in ["-1", "32", "1.5", "'1'"] {
            let args: Value =
                serde_yaml_ng::from_str(&format!("periods:\n  - monthdays: [{raw}]\n"))
                    .expect("valid YAML");
            let config =
                parse_config("time", Some(args)).expect("config accepts raw monthday values");
            let error = compile_periods("time", config.periods)
                .expect_err("invalid monthday must be rejected")
                .to_string();
            assert!(error.contains("periods[0].monthdays[0]"));
        }
    }

    #[test]
    fn test_compile_weekdays_accepts_names_and_iso_numbers() {
        let named = compile_weekdays("time", "periods[0]", Some(weekdays(&["MON", "sun"])))
            .expect("named weekdays must be accepted");
        let numbered = compile_weekdays("time", "periods[0]", Some(weekday_numbers(&[1, 7])))
            .expect("ISO weekday numbers must be accepted");
        assert_eq!(named, numbered);

        for raw in ["0", "8", "1.5", "'1'"] {
            let args: Value =
                serde_yaml_ng::from_str(&format!("periods:\n  - weekdays: [{raw}]\n"))
                    .expect("valid YAML");
            let config =
                parse_config("time", Some(args)).expect("config accepts raw weekday values");
            let error = compile_periods("time", config.periods)
                .expect_err("invalid weekday must be rejected")
                .to_string();
            assert!(error.contains("periods[0].weekdays[0]"));
        }
    }

    #[test]
    fn test_time_window_uses_half_open_boundaries() {
        let matcher = matcher(config(vec![period(Some("09:00"), Some("18:00"))]));
        assert!(!matcher.matches_timestamp(parsed_timestamp("2023-12-31T23:59:00Z")));
        assert!(matcher.matches_timestamp(parsed_timestamp("2024-01-01T09:00:00Z")));
        assert!(matcher.matches_timestamp(parsed_timestamp("2024-01-01T17:59:00Z")));
        assert!(!matcher.matches_timestamp(parsed_timestamp("2024-01-01T18:00:00Z")));
    }

    #[test]
    fn test_time_window_applies_calendar_conditions_and_ors_periods() {
        let matcher = matcher(config(vec![
            PeriodConfig {
                start: Some("09:00".to_string()),
                end: Some("18:00".to_string()),
                weekdays: Some(weekdays(&["MON"])),
                monthdays: Some(monthdays(&[1])),
            },
            PeriodConfig {
                start: None,
                end: None,
                weekdays: None,
                monthdays: Some(monthdays(&[2])),
            },
        ]));
        assert!(matcher.matches_timestamp(timestamp(1_704_099_600_000))); // Mon, Jan 1 09:00 UTC
        assert!(!matcher.matches_timestamp(timestamp(1_704_272_400_000))); // Wed, Jan 3 09:00 UTC
        assert!(matcher.matches_timestamp(timestamp(1_704_153_600_000))); // Tue, Jan 2 00:00 UTC
    }

    #[test]
    fn test_overnight_window_uses_start_day_calendar_constraints() {
        let matcher = matcher(config(vec![PeriodConfig {
            start: Some("22:00".to_string()),
            end: Some("02:00".to_string()),
            weekdays: Some(weekdays(&["mon"])),
            monthdays: Some(monthdays(&[1])),
        }]));
        assert!(matcher.matches_timestamp(parsed_timestamp("2024-01-01T22:00:00Z")));
        assert!(matcher.matches_timestamp(parsed_timestamp("2024-01-02T00:59:00Z")));
        assert!(!matcher.matches_timestamp(parsed_timestamp("2024-01-02T02:00:00Z")));
    }

    #[test]
    fn test_configured_timezone_changes_wall_clock_match() {
        let utc = matcher(config(vec![period(Some("08:00"), Some("09:00"))]));
        let shanghai = matcher(TimeConfig {
            timezone: Some("Asia/Shanghai".to_string()),
            periods: vec![period(Some("08:00"), Some("09:00"))],
        });
        let midnight_utc = timestamp(1_704_067_200_000); // 2024-01-01T00:00Z
        assert!(!utc.matches_timestamp(midnight_utc));
        assert!(shanghai.matches_timestamp(midnight_utc));
    }

    #[test]
    fn test_dst_uses_observed_local_wall_clock_time() {
        let repeated_hour = matcher(TimeConfig {
            timezone: Some("America/New_York".to_string()),
            periods: vec![period(Some("01:00"), Some("02:00"))],
        });
        assert!(repeated_hour.matches_timestamp(parsed_timestamp("2024-11-03T05:30:00Z")));
        assert!(repeated_hour.matches_timestamp(parsed_timestamp("2024-11-03T06:30:00Z")));

        let skipped_hour = matcher(TimeConfig {
            timezone: Some("America/New_York".to_string()),
            periods: vec![period(Some("02:00"), Some("03:00"))],
        });
        assert!(!skipped_hour.matches_timestamp(parsed_timestamp("2024-03-10T06:59:00Z")));
        assert!(!skipped_hour.matches_timestamp(parsed_timestamp("2024-03-10T07:00:00Z")));
    }

    #[test]
    fn test_quick_setup_requires_one_daily_window() {
        let factory = TimeFactory;
        assert!(
            factory
                .quick_setup("time", Some("09:00-18:00".to_string()))
                .is_ok()
        );
        assert!(factory.quick_setup("time", None).is_err());
        assert!(
            factory
                .quick_setup("time", Some("09:00-18:00 20:00-21:00".to_string()))
                .is_err()
        );
    }
}

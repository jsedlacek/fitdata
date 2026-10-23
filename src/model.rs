//! Normalizes raw API data points into flat, typed records and per-day metrics.
//!
//! All timestamps are converted to the user's local wall-clock time using the
//! UTC offset stored with each point, so days split at local midnight.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Timelike};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct SleepStage {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub stage: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Sleep {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub main: bool,
    pub minutes_asleep: Option<i64>,
    pub minutes_awake: Option<i64>,
    /// Minutes per stage type (DEEP, LIGHT, REM, AWAKE).
    pub stage_minutes: BTreeMap<String, i64>,
    pub stages: Vec<SleepStage>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Exercise {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub kind: String,
    pub name: String,
    pub active_minutes: Option<f64>,
    pub avg_hr: Option<f64>,
    pub kcal: Option<f64>,
    pub azm: Option<f64>,
}

/// One row per local calendar day; every field optional because devices and
/// days differ in what they report.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DailyMetrics {
    pub date: Option<NaiveDate>,
    pub resting_hr: Option<f64>,
    pub hrv_ms: Option<f64>,
    pub hrv_deep_rmssd_ms: Option<f64>,
    pub nonrem_hr: Option<f64>,
    pub spo2_avg: Option<f64>,
    pub spo2_low: Option<f64>,
    pub spo2_high: Option<f64>,
    pub respiratory_rate: Option<f64>,
    pub nightly_temp_c: Option<f64>,
    pub steps: Option<f64>,
    pub active_kcal: Option<f64>,
    pub azm: Option<f64>,
    pub hr_min: Option<f64>,
    pub hr_avg: Option<f64>,
    pub hr_max: Option<f64>,
    pub sleep_minutes: Option<f64>,
    pub sleep_awake_minutes: Option<f64>,
    pub sleep_deep_minutes: Option<f64>,
    pub sleep_light_minutes: Option<f64>,
    pub sleep_rem_minutes: Option<f64>,
    pub sleep_start: Option<NaiveDateTime>,
    pub sleep_end: Option<NaiveDateTime>,
    pub exercise_minutes: Option<f64>,
}

#[derive(Default)]
pub struct Dataset {
    /// (local time, bpm), sorted.
    pub heart_rate: Vec<(NaiveDateTime, f64)>,
    /// Per-minute intervals: (local start, value), sorted.
    pub steps: Vec<(NaiveDateTime, f64)>,
    pub active_kcal: Vec<(NaiveDateTime, f64)>,
    pub azm: Vec<(NaiveDateTime, f64)>,
    pub spo2: Vec<(NaiveDateTime, f64)>,
    /// (local time, RMSSD ms)
    pub hrv: Vec<(NaiveDateTime, f64)>,
    pub sleep: Vec<Sleep>,
    pub exercise: Vec<Exercise>,
    pub daily: BTreeMap<NaiveDate, DailyMetrics>,
}

/// `"7200s"` -> 2 h
fn offset(v: &Value) -> Duration {
    v.as_str()
        .and_then(|s| s.trim_end_matches('s').parse::<f64>().ok())
        .map(|s| Duration::seconds(s as i64))
        .unwrap_or_default()
}

fn local(ts: &Value, off: &Value) -> Option<NaiveDateTime> {
    let t = DateTime::parse_from_rfc3339(ts.as_str()?).ok()?;
    Some(t.naive_utc() + offset(off))
}

/// Numbers arrive as JSON numbers or as strings (int64 fields, "NaN").
fn num(v: &Value) -> Option<f64> {
    let n = match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }?;
    n.is_finite().then_some(n)
}

fn date(v: &Value) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(
        v["year"].as_i64()? as i32,
        v["month"].as_u64()? as u32,
        v["day"].as_u64()? as u32,
    )
}

fn sample_time(p: &Value) -> Option<NaiveDateTime> {
    local(
        &p["sampleTime"]["physicalTime"],
        &p["sampleTime"]["utcOffset"],
    )
}

fn interval_start(p: &Value) -> Option<NaiveDateTime> {
    local(
        &p["interval"]["startTime"],
        &p["interval"]["startUtcOffset"],
    )
}

fn interval_end(p: &Value) -> Option<NaiveDateTime> {
    local(&p["interval"]["endTime"], &p["interval"]["endUtcOffset"])
}

/// Every point of a data type, from all `raw/{type}/*.jsonl` files, unwrapped
/// to the payload object (e.g. the value of `"heartRate"`).
fn read_type(dir: &Path, data_type: &str, key: &str) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir.join("raw").join(data_type)) else {
        return Ok(out);
    };
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "jsonl") {
            for line in std::fs::read_to_string(&path)?
                .lines()
                .filter(|l| !l.is_empty())
            {
                let mut v: Value = serde_json::from_str(line)?;
                out.push(v[key].take());
            }
        }
    }
    Ok(out)
}

fn series(
    points: &[Value],
    time: fn(&Value) -> Option<NaiveDateTime>,
    field: &str,
) -> Vec<(NaiveDateTime, f64)> {
    let mut s: Vec<_> = points
        .iter()
        .filter_map(|p| Some((time(p)?, num(&p[field])?)))
        .collect();
    s.sort_by_key(|(t, _)| *t);
    s.dedup_by_key(|(t, _)| *t);
    s
}

impl Dataset {
    /// Loads everything under a local data directory (the prototype keeps
    /// it all in memory; production will query BigQuery instead).
    pub fn load(dir: &Path) -> Result<Self> {
        let mut ds = Dataset {
            heart_rate: series(
                &read_type(dir, "heart-rate", "heartRate")?,
                sample_time,
                "beatsPerMinute",
            ),
            steps: series(&read_type(dir, "steps", "steps")?, interval_start, "count"),
            active_kcal: series(
                &read_type(dir, "active-energy-burned", "activeEnergyBurned")?,
                interval_start,
                "kcal",
            ),
            azm: series(
                &read_type(dir, "active-zone-minutes", "activeZoneMinutes")?,
                interval_start,
                "activeZoneMinutes",
            ),
            spo2: series(
                &read_type(dir, "oxygen-saturation", "oxygenSaturation")?,
                sample_time,
                "percentage",
            ),
            hrv: series(
                &read_type(dir, "heart-rate-variability", "heartRateVariability")?,
                sample_time,
                "rootMeanSquareOfSuccessiveDifferencesMilliseconds",
            ),
            ..Default::default()
        };

        for p in read_type(dir, "sleep", "sleep")? {
            let (Some(start), Some(end)) = (interval_start(&p), interval_end(&p)) else {
                continue;
            };
            let stages = p["stages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| {
                    Some(SleepStage {
                        start: local(&s["startTime"], &s["startUtcOffset"])?,
                        end: local(&s["endTime"], &s["endUtcOffset"])?,
                        stage: s["type"].as_str()?.to_string(),
                    })
                })
                .collect();
            let stage_minutes = p["summary"]["stagesSummary"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| Some((s["type"].as_str()?.to_string(), num(&s["minutes"])? as i64)))
                .collect();
            ds.sleep.push(Sleep {
                start,
                end,
                main: p["metadata"]["mainSleep"].as_bool().unwrap_or(false),
                minutes_asleep: num(&p["summary"]["minutesAsleep"]).map(|m| m as i64),
                minutes_awake: num(&p["summary"]["minutesAwake"]).map(|m| m as i64),
                stage_minutes,
                stages,
            });
        }
        ds.sleep.sort_by_key(|s| s.start);
        ds.sleep.dedup_by_key(|s| s.start);

        for p in read_type(dir, "exercise", "exercise")? {
            let (Some(start), Some(end)) = (interval_start(&p), interval_end(&p)) else {
                continue;
            };
            let m = &p["metricsSummary"];
            ds.exercise.push(Exercise {
                start,
                end,
                kind: p["exerciseType"].as_str().unwrap_or("UNKNOWN").to_string(),
                name: p["displayName"].as_str().unwrap_or_default().to_string(),
                active_minutes: p["activeDuration"]
                    .as_str()
                    .and_then(|d| d.trim_end_matches('s').parse::<f64>().ok())
                    .map(|s| s / 60.0),
                avg_hr: num(&m["averageHeartRateBeatsPerMinute"]),
                kcal: num(&m["caloriesKcal"]),
                azm: num(&m["activeZoneMinutes"]),
            });
        }
        ds.exercise.sort_by_key(|e| e.start);

        ds.build_daily(dir)?;
        Ok(ds)
    }

    fn day(&mut self, d: NaiveDate) -> &mut DailyMetrics {
        self.daily.entry(d).or_insert_with(|| DailyMetrics {
            date: Some(d),
            ..Default::default()
        })
    }

    fn build_daily(&mut self, dir: &Path) -> Result<()> {
        // Daily summaries computed by Google.
        for p in read_type(dir, "daily-resting-heart-rate", "dailyRestingHeartRate")? {
            if let Some(d) = date(&p["date"]) {
                self.day(d).resting_hr = num(&p["beatsPerMinute"]);
            }
        }
        for p in read_type(
            dir,
            "daily-heart-rate-variability",
            "dailyHeartRateVariability",
        )? {
            if let Some(d) = date(&p["date"]) {
                let m = self.day(d);
                m.hrv_ms = num(&p["averageHeartRateVariabilityMilliseconds"]);
                m.hrv_deep_rmssd_ms =
                    num(&p["deepSleepRootMeanSquareOfSuccessiveDifferencesMilliseconds"]);
                m.nonrem_hr = num(&p["nonRemHeartRateBeatsPerMinute"]);
            }
        }
        for p in read_type(dir, "daily-oxygen-saturation", "dailyOxygenSaturation")? {
            if let Some(d) = date(&p["date"]) {
                let m = self.day(d);
                m.spo2_avg = num(&p["averagePercentage"]);
                m.spo2_low = num(&p["lowerBoundPercentage"]);
                m.spo2_high = num(&p["upperBoundPercentage"]);
            }
        }
        for p in read_type(dir, "daily-respiratory-rate", "dailyRespiratoryRate")? {
            if let Some(d) = date(&p["date"]) {
                self.day(d).respiratory_rate = num(&p["breathsPerMinute"]);
            }
        }
        for p in read_type(
            dir,
            "daily-sleep-temperature-derivations",
            "dailySleepTemperatureDerivations",
        )? {
            if let Some(d) = date(&p["date"]) {
                self.day(d).nightly_temp_c = num(&p["nightlyTemperatureCelsius"]);
            }
        }

        // Aggregates of intraday data.
        let sum_by_day = |s: &[(NaiveDateTime, f64)]| {
            let mut m: BTreeMap<NaiveDate, f64> = BTreeMap::new();
            for (t, v) in s {
                *m.entry(t.date()).or_default() += v;
            }
            m
        };
        for (d, v) in sum_by_day(&self.steps) {
            self.day(d).steps = Some(v);
        }
        for (d, v) in sum_by_day(&self.active_kcal) {
            self.day(d).active_kcal = Some(v.round());
        }
        for (d, v) in sum_by_day(&self.azm) {
            self.day(d).azm = Some(v);
        }
        let mut hr: BTreeMap<NaiveDate, (f64, f64, f64, usize)> = BTreeMap::new();
        for (t, v) in &self.heart_rate {
            let e = hr.entry(t.date()).or_insert((f64::MAX, f64::MIN, 0.0, 0));
            e.0 = e.0.min(*v);
            e.1 = e.1.max(*v);
            e.2 += v;
            e.3 += 1;
        }
        for (d, (lo, hi, sum, n)) in hr {
            let m = self.day(d);
            m.hr_min = Some(lo);
            m.hr_max = Some(hi);
            m.hr_avg = Some((sum / n as f64 * 10.0).round() / 10.0);
        }

        // A night's sleep belongs to the day you wake up on.
        let nights: Vec<_> = self
            .daily
            .keys()
            .copied()
            .chain(self.sleep.iter().map(|s| s.end.date()))
            .collect();
        for d in nights {
            let Some(n) = self.night_before(d) else {
                continue;
            };
            let m = self.day(d);
            m.sleep_minutes = n.minutes_asleep.map(|v| v as f64);
            m.sleep_awake_minutes = n.minutes_awake.map(|v| v as f64);
            m.sleep_deep_minutes = n.stage_minutes.get("DEEP").map(|v| *v as f64);
            m.sleep_light_minutes = n.stage_minutes.get("LIGHT").map(|v| *v as f64);
            m.sleep_rem_minutes = n.stage_minutes.get("REM").map(|v| *v as f64);
            m.sleep_start = Some(n.start);
            m.sleep_end = Some(n.end);
        }
        let exercises = self.exercise.clone();
        for e in &exercises {
            let m = self.day(e.start.date());
            m.exercise_minutes = Some(
                m.exercise_minutes.unwrap_or(0.0) + (e.end - e.start).num_seconds() as f64 / 60.0,
            );
        }
        Ok(())
    }

    /// The night before `d`: all sleep sessions that ended on `d` before 14:00,
    /// merged. Google sometimes splits one night into several sessions (e.g.
    /// after waking up for an hour); later sessions are naps and don't count.
    pub fn night_before(&self, d: NaiveDate) -> Option<Sleep> {
        let cutoff = d.and_hms_opt(14, 0, 0)?;
        let parts: Vec<&Sleep> = self
            .sleep
            .iter()
            .filter(|s| s.end.date() == d && s.end < cutoff)
            .collect();
        let first = parts.first()?;
        let mut night = Sleep {
            start: first.start,
            end: first.end,
            main: true,
            minutes_asleep: None,
            minutes_awake: None,
            stage_minutes: BTreeMap::new(),
            stages: Vec::new(),
        };
        let add = |a: Option<i64>, b: Option<i64>| match (a, b) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
        };
        for p in parts {
            night.start = night.start.min(p.start);
            night.end = night.end.max(p.end);
            night.minutes_asleep = add(night.minutes_asleep, p.minutes_asleep);
            night.minutes_awake = add(night.minutes_awake, p.minutes_awake);
            for (k, v) in &p.stage_minutes {
                *night.stage_minutes.entry(k.clone()).or_default() += v;
            }
            night.stages.extend(p.stages.iter().cloned());
        }
        night.stages.sort_by_key(|s| s.start);
        Some(night)
    }
}

/// Points within `[from, to)` of a time-sorted series.
pub fn window(
    s: &[(NaiveDateTime, f64)],
    from: NaiveDateTime,
    to: NaiveDateTime,
) -> &[(NaiveDateTime, f64)] {
    let a = s.partition_point(|(t, _)| *t < from);
    let b = s.partition_point(|(t, _)| *t < to);
    &s[a..b]
}

/// Buckets a series into `minutes`-wide bins: (bin start, min, mean, max).
pub fn bin_stats(s: &[(NaiveDateTime, f64)], minutes: u32) -> Vec<(NaiveDateTime, f64, f64, f64)> {
    let mut out: Vec<(NaiveDateTime, f64, f64, f64, usize)> = Vec::new();
    for (t, v) in s {
        let bin = t.with_second(0).unwrap().with_nanosecond(0).unwrap()
            - Duration::minutes((t.minute() % minutes) as i64);
        match out.last_mut() {
            Some(last) if last.0 == bin => {
                last.1 = last.1.min(*v);
                last.3 = last.3.max(*v);
                last.2 += v;
                last.4 += 1;
            }
            _ => out.push((bin, *v, *v, *v, 1)),
        }
    }
    out.into_iter()
        .map(|(t, lo, sum, hi, n)| (t, lo, (sum / n as f64 * 10.0).round() / 10.0, hi))
        .collect()
}

/// Sums a series into `minutes`-wide bins: (bin start, total).
pub fn bin_sum(s: &[(NaiveDateTime, f64)], minutes: u32) -> Vec<(NaiveDateTime, f64)> {
    let mut out: Vec<(NaiveDateTime, f64)> = Vec::new();
    for (t, v) in s {
        let bin = t.with_second(0).unwrap() - Duration::minutes((t.minute() % minutes) as i64);
        match out.last_mut() {
            Some(last) if last.0 == bin => last.1 += v,
            _ => out.push((bin, *v)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn converts_to_local_time() {
        let t = local(&json!("2026-10-23T17:55:56Z"), &json!("7200s")).unwrap();
        assert_eq!(t.to_string(), "2026-10-23 19:55:56");
    }

    #[test]
    fn parses_string_and_nan_numbers() {
        assert_eq!(num(&json!("61")), Some(61.0));
        assert_eq!(num(&json!(96.6)), Some(96.6));
        assert_eq!(num(&json!("NaN")), None);
    }

    #[test]
    fn merges_split_night_and_skips_naps() {
        let t = |s: &str| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap();
        let sleep = |a, b, mins, main| Sleep {
            start: t(a),
            end: t(b),
            main,
            minutes_asleep: Some(mins),
            minutes_awake: None,
            stage_minutes: BTreeMap::from([("DEEP".to_string(), mins / 4)]),
            stages: Vec::new(),
        };
        let ds = Dataset {
            sleep: vec![
                sleep("2026-10-21 23:14", "2026-10-22 03:13", 233, true),
                sleep("2026-10-22 05:18", "2026-10-22 08:57", 197, false),
                sleep("2026-10-22 15:00", "2026-10-22 15:40", 35, false),
            ],
            ..Default::default()
        };
        let n = ds.night_before(t("2026-10-22 00:00").date()).unwrap();
        assert_eq!(
            (n.start, n.end),
            (t("2026-10-21 23:14"), t("2026-10-22 08:57"))
        );
        assert_eq!(n.minutes_asleep, Some(430));
        assert_eq!(n.stage_minutes["DEEP"], 58 + 49);
    }

    #[test]
    fn bins_by_minutes() {
        let t = |s: &str| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap();
        let s = vec![
            (t("2026-10-23 10:01:10"), 60.0),
            (t("2026-10-23 10:14:00"), 80.0),
            (t("2026-10-23 10:15:00"), 70.0),
        ];
        let b = bin_stats(&s, 15);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0], (t("2026-10-23 10:00:00"), 60.0, 70.0, 80.0));
    }
}

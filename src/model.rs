//! Normalizes raw API data points into flat, typed records and per-day metrics.
//!
//! All timestamps are converted to the user's local wall-clock time using the
//! UTC offset stored with each point, so days split at local midnight.

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime};
use serde::Serialize;
use serde_json::{Value, json};

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
    /// Short awakenings (a minute or two) inside light sleep; Google Health
    /// shows them as restless sleep.
    pub restless: Vec<Span>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Span {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
}

impl Sleep {
    pub fn restless_minutes(&self) -> i64 {
        self.restless
            .iter()
            .map(|s| (s.end - s.start).num_minutes())
            .sum()
    }
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

/// Payload objects of raw API data points per data type id (see `sync::read_raw`).
pub type Raw = HashMap<String, Vec<Value>>;

/// Takes all points of a data type out of `raw` (each type is read once).
fn read_type(raw: &mut Raw, data_type: &str) -> Result<Vec<Value>> {
    Ok(raw.remove(data_type).unwrap_or_default())
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
    /// Normalizes raw points (typically a few days of them, see
    /// `sync::read_raw`) into typed series and per-day metrics.
    pub fn from_raw(mut raw: Raw) -> Result<Self> {
        let raw = &mut raw;
        let mut ds = Dataset {
            heart_rate: series(
                &read_type(raw, "heart-rate")?,
                sample_time,
                "beatsPerMinute",
            ),
            steps: series(&read_type(raw, "steps")?, interval_start, "count"),
            active_kcal: series(
                &read_type(raw, "active-energy-burned")?,
                interval_start,
                "kcal",
            ),
            azm: series(
                &read_type(raw, "active-zone-minutes")?,
                interval_start,
                "activeZoneMinutes",
            ),
            spo2: series(
                &read_type(raw, "oxygen-saturation")?,
                sample_time,
                "percentage",
            ),
            hrv: series(
                &read_type(raw, "heart-rate-variability")?,
                sample_time,
                "rootMeanSquareOfSuccessiveDifferencesMilliseconds",
            ),
            ..Default::default()
        };

        for p in read_type(raw, "sleep")? {
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
            let restless = p["shortAwakenings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| {
                    Some(Span {
                        start: local(&s["startTime"], &s["startUtcOffset"])?,
                        end: local(&s["endTime"], &s["endUtcOffset"])?,
                    })
                })
                .collect();
            ds.sleep.push(Sleep {
                start,
                end,
                main: p["metadata"]["mainSleep"].as_bool().unwrap_or(false),
                minutes_asleep: num(&p["summary"]["minutesAsleep"]).map(|m| m as i64),
                minutes_awake: num(&p["summary"]["minutesAwake"]).map(|m| m as i64),
                stage_minutes,
                stages,
                restless,
            });
        }
        ds.sleep.sort_by_key(|s| s.start);
        ds.sleep.dedup_by_key(|s| s.start);

        for p in read_type(raw, "exercise")? {
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

        ds.build_daily(raw)?;
        Ok(ds)
    }

    fn day(&mut self, d: NaiveDate) -> &mut DailyMetrics {
        self.daily.entry(d).or_insert_with(|| DailyMetrics {
            date: Some(d),
            ..Default::default()
        })
    }

    fn build_daily(&mut self, raw: &mut Raw) -> Result<()> {
        // Daily summaries computed by Google.
        for p in read_type(raw, "daily-resting-heart-rate")? {
            if let Some(d) = date(&p["date"]) {
                self.day(d).resting_hr = num(&p["beatsPerMinute"]);
            }
        }
        for p in read_type(raw, "daily-heart-rate-variability")? {
            if let Some(d) = date(&p["date"]) {
                let m = self.day(d);
                m.hrv_ms = num(&p["averageHeartRateVariabilityMilliseconds"]);
                m.hrv_deep_rmssd_ms =
                    num(&p["deepSleepRootMeanSquareOfSuccessiveDifferencesMilliseconds"]);
                m.nonrem_hr = num(&p["nonRemHeartRateBeatsPerMinute"]);
            }
        }
        for p in read_type(raw, "daily-oxygen-saturation")? {
            if let Some(d) = date(&p["date"]) {
                let m = self.day(d);
                m.spo2_avg = num(&p["averagePercentage"]);
                m.spo2_low = num(&p["lowerBoundPercentage"]);
                m.spo2_high = num(&p["upperBoundPercentage"]);
            }
        }
        for p in read_type(raw, "daily-respiratory-rate")? {
            if let Some(d) = date(&p["date"]) {
                self.day(d).respiratory_rate = num(&p["breathsPerMinute"]);
            }
        }
        for p in read_type(raw, "daily-sleep-temperature-derivations")? {
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
    /// Gaps between the sessions count as awake, as in Google Health.
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
            restless: Vec::new(),
        };
        let add = |a: Option<i64>, b: Option<i64>| match (a, b) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
        };
        for p in parts {
            if p.start > night.end {
                let gap = (p.start - night.end).num_minutes();
                night.minutes_awake = add(night.minutes_awake, Some(gap));
                *night.stage_minutes.entry("AWAKE".to_string()).or_default() += gap;
                night.stages.push(SleepStage {
                    start: night.end,
                    end: p.start,
                    stage: "AWAKE".to_string(),
                });
            }
            night.start = night.start.min(p.start);
            night.end = night.end.max(p.end);
            night.minutes_asleep = add(night.minutes_asleep, p.minutes_asleep);
            night.minutes_awake = add(night.minutes_awake, p.minutes_awake);
            for (k, v) in &p.stage_minutes {
                *night.stage_minutes.entry(k.clone()).or_default() += v;
            }
            night.stages.extend(p.stages.iter().cloned());
            night.restless.extend(p.restless.iter().cloned());
        }
        night.stages.sort_by_key(|s| s.start);
        night.restless.sort_by_key(|s| s.start);
        Some(night)
    }
}

/// Rows for the BigQuery tables (see deploy/schema/), limited to local dates
/// in `[from, to]`. Each row has a `date` column the tables are partitioned by.
pub fn table_rows(ds: &Dataset, from: NaiveDate, to: NaiveDate) -> Vec<(&'static str, Vec<Value>)> {
    let in_range = |t: &NaiveDateTime| (from..=to).contains(&t.date());
    let samples = |s: &[(NaiveDateTime, f64)], field: &str| -> Vec<Value> {
        s.iter()
            .filter(|(t, _)| in_range(t))
            .map(|(t, v)| json!({ "date": t.date(), "ts": t, field: v }))
            .collect()
    };

    // Per-minute activity, merged from three interval series.
    let mut minutes: BTreeMap<NaiveDateTime, [Option<f64>; 3]> = BTreeMap::new();
    for (i, series) in [&ds.steps, &ds.active_kcal, &ds.azm]
        .into_iter()
        .enumerate()
    {
        for (t, v) in series.iter().filter(|(t, _)| in_range(t)) {
            minutes.entry(*t).or_default()[i] = Some(*v);
        }
    }
    let activity = minutes
        .into_iter()
        .map(|(t, [steps, kcal, azm])| json!({ "date": t.date(), "ts": t, "steps": steps, "active_kcal": kcal, "azm": azm }))
        .collect();

    let nights = from
        .iter_days()
        .take_while(|d| *d <= to)
        .filter_map(|d| {
            let n = ds.night_before(d)?;
            let m = |k: &str| n.stage_minutes.get(k).copied();
            Some(json!({
                "date": d, "start": n.start, "end": n.end,
                "minutes_asleep": n.minutes_asleep, "minutes_awake": n.minutes_awake,
                "deep_minutes": m("DEEP"), "light_minutes": m("LIGHT"),
                "rem_minutes": m("REM"), "awake_minutes": m("AWAKE"),
                "restless_minutes": n.restless_minutes(),
                "stages": n.stages, "restless": n.restless,
            }))
        })
        .collect();

    let exercises = ds
        .exercise
        .iter()
        .filter(|e| in_range(&e.start))
        .map(|e| {
            let mut v = serde_json::to_value(e).unwrap_or_default();
            v["date"] = json!(e.start.date());
            v
        })
        .collect();

    let daily = ds
        .daily
        .range(from..=to)
        .map(|(_, m)| serde_json::to_value(m).unwrap_or_default())
        .collect();

    vec![
        ("heart_rate", samples(&ds.heart_rate, "bpm")),
        ("activity", activity),
        ("spo2", samples(&ds.spo2, "pct")),
        ("hrv", samples(&ds.hrv, "rmssd_ms")),
        ("nights", nights),
        ("exercises", exercises),
        ("daily", daily),
    ]
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
            restless: vec![Span {
                start: t(a),
                end: t(a) + Duration::minutes(2),
            }],
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
        // 03:13–05:18 between the two sessions is awake.
        assert_eq!(n.minutes_awake, Some(125));
        assert_eq!(n.stage_minutes["AWAKE"], 125);
        assert_eq!(n.stages.len(), 1);
        assert_eq!(
            (n.stages[0].start, n.stages[0].end),
            (t("2026-10-22 03:13"), t("2026-10-22 05:18"))
        );
        assert_eq!(n.restless_minutes(), 4);
    }
}

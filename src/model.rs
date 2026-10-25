//! Normalizes parsed API data points (`health.rs`) into typed series, per-day
//! metrics and the rows of the Parquet tables (`tables.rs`).
//!
//! All timestamps are converted to the user's local wall-clock time using the
//! UTC offset stored with each point, so days split at local midnight.

use std::collections::BTreeMap;

use chrono::{Duration, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};

use crate::health::{Points, StageType};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SleepStage {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub stage: StageType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
}

#[derive(Debug, Clone)]
pub struct Sleep {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub minutes_asleep: Option<i64>,
    pub minutes_awake: Option<i64>,
    pub stage_minutes: BTreeMap<StageType, i64>,
    pub stages: Vec<SleepStage>,
    /// Short awakenings (a minute or two) inside light sleep; Google Health
    /// shows them as restless sleep.
    pub restless: Vec<Span>,
}

impl Sleep {
    pub fn restless_minutes(&self) -> i64 {
        self.restless
            .iter()
            .map(|s| (s.end - s.start).num_minutes())
            .sum()
    }
}

/// A row of a table; `TABLE` is its name and the file name of its schema
/// in schema/.
pub trait Row: Serialize {
    const TABLE: &'static str;
}

/// Row of `exercises`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exercise {
    pub date: NaiveDate,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub kind: String,
    pub name: String,
    pub active_minutes: Option<f64>,
    pub avg_hr: Option<f64>,
    pub kcal: Option<f64>,
    pub azm: Option<f64>,
}

impl Row for Exercise {
    const TABLE: &'static str = "exercises";
}

/// Row of `daily`: one per local calendar day; every metric optional because
/// devices and days differ in what they report.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DailyMetrics {
    pub date: NaiveDate,
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

impl Row for DailyMetrics {
    const TABLE: &'static str = "daily";
}

#[derive(Debug, Serialize)]
pub struct HeartRateRow {
    pub date: NaiveDate,
    pub ts: NaiveDateTime,
    pub bpm: f64,
}

impl Row for HeartRateRow {
    const TABLE: &'static str = "heart_rate";
}

#[derive(Debug, Serialize)]
pub struct Spo2Row {
    pub date: NaiveDate,
    pub ts: NaiveDateTime,
    pub pct: f64,
}

impl Row for Spo2Row {
    const TABLE: &'static str = "spo2";
}

#[derive(Debug, Serialize)]
pub struct HrvRow {
    pub date: NaiveDate,
    pub ts: NaiveDateTime,
    pub rmssd_ms: f64,
}

impl Row for HrvRow {
    const TABLE: &'static str = "hrv";
}

/// Row of `activity`: one minute, merged from three interval series.
#[derive(Debug, Serialize)]
pub struct ActivityRow {
    pub date: NaiveDate,
    pub ts: NaiveDateTime,
    pub steps: Option<f64>,
    pub active_kcal: Option<f64>,
    pub azm: Option<f64>,
}

impl Row for ActivityRow {
    const TABLE: &'static str = "activity";
}

/// Row of `nights`: the night that ended on `date` (see `Dataset::night_before`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NightRow {
    pub date: NaiveDate,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub minutes_asleep: Option<i64>,
    pub minutes_awake: Option<i64>,
    pub deep_minutes: Option<i64>,
    pub light_minutes: Option<i64>,
    pub rem_minutes: Option<i64>,
    pub awake_minutes: Option<i64>,
    /// NULL in rows loaded before restless sleep was tracked.
    pub restless_minutes: Option<i64>,
    pub stages: Vec<SleepStage>,
    pub restless: Vec<Span>,
}

impl Row for NightRow {
    const TABLE: &'static str = "nights";
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

/// Longest wake-up between two sleep sessions that still makes them one night.
const MAX_NIGHT_GAP: Duration = Duration::hours(3);

/// Sorted values with a time; points without a value are left out, and of
/// several points at the same time the first one is kept.
fn series<T>(
    points: &[T],
    point: impl Fn(&T) -> (NaiveDateTime, Option<f64>),
) -> Vec<(NaiveDateTime, f64)> {
    let mut s: Vec<_> = points
        .iter()
        .filter_map(|p| match point(p) {
            (t, Some(v)) => Some((t, v)),
            (_, None) => None,
        })
        .collect();
    s.sort_by_key(|(t, _)| *t);
    s.dedup_by_key(|(t, _)| *t);
    s
}

/// Whole minutes; summaries report them as integers anyway.
fn minutes(v: Option<f64>) -> Option<i64> {
    v.map(|m| m.round() as i64)
}

impl Dataset {
    /// Normalizes parsed points (typically a few days of them, see
    /// `sync::read_raw`) into typed series and per-day metrics.
    pub fn from_points(p: Points) -> Self {
        let mut ds = Dataset {
            heart_rate: series(&p.heart_rate, |x| {
                (x.sample_time.local(), x.beats_per_minute)
            }),
            steps: series(&p.steps, |x| (x.interval.local_start(), x.count)),
            active_kcal: series(&p.active_energy_burned, |x| {
                (x.interval.local_start(), x.kcal)
            }),
            azm: series(&p.active_zone_minutes, |x| {
                (x.interval.local_start(), x.active_zone_minutes)
            }),
            spo2: series(&p.oxygen_saturation, |x| {
                (x.sample_time.local(), x.percentage)
            }),
            hrv: series(&p.heart_rate_variability, |x| {
                (
                    x.sample_time.local(),
                    x.root_mean_square_of_successive_differences_milliseconds,
                )
            }),
            ..Default::default()
        };

        for s in &p.sleep {
            let span = |i: &crate::health::Interval| Span {
                start: i.local_start(),
                end: i.local_end(),
            };
            ds.sleep.push(Sleep {
                start: s.interval.local_start(),
                end: s.interval.local_end(),
                minutes_asleep: minutes(s.summary.minutes_asleep),
                minutes_awake: minutes(s.summary.minutes_awake),
                stage_minutes: s
                    .summary
                    .stages_summary
                    .iter()
                    .filter_map(|st| Some((st.stage, minutes(st.minutes)?)))
                    .collect(),
                stages: s
                    .stages
                    .iter()
                    .map(|st| {
                        let Span { start, end } = span(&st.interval);
                        SleepStage {
                            start,
                            end,
                            stage: st.stage,
                        }
                    })
                    .collect(),
                restless: s.short_awakenings.iter().map(span).collect(),
            });
        }
        ds.sleep.sort_by_key(|s| s.start);
        ds.sleep.dedup_by_key(|s| s.start);

        for e in &p.exercise {
            let (start, end) = (e.interval.local_start(), e.interval.local_end());
            let m = &e.metrics_summary;
            ds.exercise.push(Exercise {
                date: start.date(),
                start,
                end,
                kind: e.exercise_type.clone().unwrap_or_else(|| "UNKNOWN".into()),
                name: e.display_name.clone(),
                active_minutes: e
                    .active_duration
                    .map(|d| d.num_milliseconds() as f64 / 60_000.0),
                avg_hr: m.average_heart_rate_beats_per_minute,
                kcal: m.calories_kcal,
                azm: m.active_zone_minutes,
            });
        }
        ds.exercise.sort_by_key(|e| e.start);

        ds.build_daily(&p);
        ds
    }

    fn day(&mut self, d: NaiveDate) -> &mut DailyMetrics {
        self.daily.entry(d).or_insert_with(|| DailyMetrics {
            date: d,
            ..Default::default()
        })
    }

    fn build_daily(&mut self, p: &Points) {
        // Daily summaries computed by Google.
        for x in &p.daily_resting_heart_rate {
            self.day(x.date.0).resting_hr = x.beats_per_minute;
        }
        for x in &p.daily_heart_rate_variability {
            let m = self.day(x.date.0);
            m.hrv_ms = x.average_heart_rate_variability_milliseconds;
            m.hrv_deep_rmssd_ms =
                x.deep_sleep_root_mean_square_of_successive_differences_milliseconds;
            m.nonrem_hr = x.non_rem_heart_rate_beats_per_minute;
        }
        for x in &p.daily_oxygen_saturation {
            let m = self.day(x.date.0);
            m.spo2_avg = x.average_percentage;
            m.spo2_low = x.lower_bound_percentage;
            m.spo2_high = x.upper_bound_percentage;
        }
        for x in &p.daily_respiratory_rate {
            self.day(x.date.0).respiratory_rate = x.breaths_per_minute;
        }
        for x in &p.daily_sleep_temperature_derivations {
            self.day(x.date.0).nightly_temp_c = x.nightly_temperature_celsius;
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
            let stage = |s| n.stage_minutes.get(&s).map(|v| *v as f64);
            let m = self.day(d);
            m.sleep_minutes = n.minutes_asleep.map(|v| v as f64);
            m.sleep_awake_minutes = n.minutes_awake.map(|v| v as f64);
            m.sleep_deep_minutes = stage(StageType::Deep);
            m.sleep_light_minutes = stage(StageType::Light);
            m.sleep_rem_minutes = stage(StageType::Rem);
            m.sleep_start = Some(n.start);
            m.sleep_end = Some(n.end);
        }
        let exercises = self.exercise.clone();
        for e in &exercises {
            let m = self.day(e.date);
            m.exercise_minutes = Some(
                m.exercise_minutes.unwrap_or(0.0) + (e.end - e.start).num_seconds() as f64 / 60.0,
            );
        }
    }

    /// The night before `d`: the first sleep session that ended on `d` before
    /// 14:00, merged with the sessions that follow it within `MAX_NIGHT_GAP`.
    /// Google sometimes splits one night into several sessions (e.g. after
    /// waking up for an hour); a later session after a longer gap is a nap and
    /// doesn't count. Gaps between merged sessions count as awake, as in Google Health.
    pub fn night_before(&self, d: NaiveDate) -> Option<Sleep> {
        let cutoff = d.and_hms_opt(14, 0, 0)?;
        let mut parts: Vec<&Sleep> = Vec::new();
        for s in self
            .sleep
            .iter()
            .filter(|s| s.end.date() == d && s.end < cutoff)
        {
            let end = parts.iter().map(|p| p.end).max();
            if end.is_some_and(|e| s.start - e > MAX_NIGHT_GAP) {
                break;
            }
            parts.push(s);
        }
        let first = parts.first()?;
        let mut night = Sleep {
            start: first.start,
            end: first.end,
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
                *night.stage_minutes.entry(StageType::Awake).or_default() += gap;
                night.stages.push(SleepStage {
                    start: night.end,
                    end: p.start,
                    stage: StageType::Awake,
                });
            }
            night.start = night.start.min(p.start);
            night.end = night.end.max(p.end);
            night.minutes_asleep = add(night.minutes_asleep, p.minutes_asleep);
            night.minutes_awake = add(night.minutes_awake, p.minutes_awake);
            for (k, v) in &p.stage_minutes {
                *night.stage_minutes.entry(*k).or_default() += v;
            }
            night.stages.extend(p.stages.iter().cloned());
            night.restless.extend(p.restless.iter().cloned());
        }
        night.stages.sort_by_key(|s| s.start);
        night.restless.sort_by_key(|s| s.start);
        Some(night)
    }
}

/// Rows of every table for local dates in `[from, to]`.
pub struct Tables {
    pub heart_rate: Vec<HeartRateRow>,
    pub activity: Vec<ActivityRow>,
    pub spo2: Vec<Spo2Row>,
    pub hrv: Vec<HrvRow>,
    pub nights: Vec<NightRow>,
    pub exercises: Vec<Exercise>,
    pub daily: Vec<DailyMetrics>,
}

pub fn table_rows(ds: &Dataset, from: NaiveDate, to: NaiveDate) -> Tables {
    let in_range = |t: &NaiveDateTime| (from..=to).contains(&t.date());
    let samples = |s: &[(NaiveDateTime, f64)]| {
        s.iter()
            .filter(|(t, _)| in_range(t))
            .map(|&(t, v)| (t.date(), t, v))
            .collect::<Vec<_>>()
    };

    let mut minutes: BTreeMap<NaiveDateTime, [Option<f64>; 3]> = BTreeMap::new();
    for (i, series) in [&ds.steps, &ds.active_kcal, &ds.azm]
        .into_iter()
        .enumerate()
    {
        for (t, v) in series.iter().filter(|(t, _)| in_range(t)) {
            minutes.entry(*t).or_default()[i] = Some(*v);
        }
    }

    let nights = from
        .iter_days()
        .take_while(|d| *d <= to)
        .filter_map(|date| {
            let n = ds.night_before(date)?;
            let m = |k| n.stage_minutes.get(&k).copied();
            Some(NightRow {
                date,
                start: n.start,
                end: n.end,
                minutes_asleep: n.minutes_asleep,
                minutes_awake: n.minutes_awake,
                deep_minutes: m(StageType::Deep),
                light_minutes: m(StageType::Light),
                rem_minutes: m(StageType::Rem),
                awake_minutes: m(StageType::Awake),
                restless_minutes: Some(n.restless_minutes()),
                stages: n.stages,
                restless: n.restless,
            })
        })
        .collect();

    Tables {
        heart_rate: samples(&ds.heart_rate)
            .into_iter()
            .map(|(date, ts, bpm)| HeartRateRow { date, ts, bpm })
            .collect(),
        activity: minutes
            .into_iter()
            .map(|(ts, [steps, active_kcal, azm])| ActivityRow {
                date: ts.date(),
                ts,
                steps,
                active_kcal,
                azm,
            })
            .collect(),
        spo2: samples(&ds.spo2)
            .into_iter()
            .map(|(date, ts, pct)| Spo2Row { date, ts, pct })
            .collect(),
        hrv: samples(&ds.hrv)
            .into_iter()
            .map(|(date, ts, rmssd_ms)| HrvRow { date, ts, rmssd_ms })
            .collect(),
        nights,
        exercises: ds
            .exercise
            .iter()
            .filter(|e| in_range(&e.start))
            .cloned()
            .collect(),
        daily: ds.daily.range(from..=to).map(|(_, m)| m.clone()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    #[test]
    fn merges_split_night_and_skips_naps() {
        let sleep = |a, b, mins| Sleep {
            start: t(a),
            end: t(b),
            minutes_asleep: Some(mins),
            minutes_awake: None,
            stage_minutes: BTreeMap::from([(StageType::Deep, mins / 4)]),
            stages: Vec::new(),
            restless: vec![Span {
                start: t(a),
                end: t(a) + Duration::minutes(2),
            }],
        };
        let ds = Dataset {
            sleep: vec![
                sleep("2026-10-21 23:14", "2026-10-22 03:13", 233),
                sleep("2026-10-22 05:18", "2026-10-22 08:57", 197),
                // Naps: more than 3 hours after waking up, or after 14:00.
                sleep("2026-10-22 12:30", "2026-10-22 13:10", 38),
                sleep("2026-10-22 15:00", "2026-10-22 15:40", 35),
            ],
            ..Default::default()
        };
        let n = ds.night_before(t("2026-10-22 00:00").date()).unwrap();
        assert_eq!(
            (n.start, n.end),
            (t("2026-10-21 23:14"), t("2026-10-22 08:57"))
        );
        assert_eq!(n.minutes_asleep, Some(430));
        assert_eq!(n.stage_minutes[&StageType::Deep], 58 + 49);
        // 03:13–05:18 between the two sessions is awake.
        assert_eq!(n.minutes_awake, Some(125));
        assert_eq!(n.stage_minutes[&StageType::Awake], 125);
        assert_eq!(n.stages.len(), 1);
        assert_eq!(
            (n.stages[0].start, n.stages[0].end),
            (t("2026-10-22 03:13"), t("2026-10-22 05:18"))
        );
        assert_eq!(n.restless_minutes(), 4);
    }

    /// Column paths of a schema in schema/, nested records as `a.b`.
    fn schema_columns(table: &str) -> Vec<String> {
        fn walk(fields: &Value, prefix: &str, out: &mut Vec<String>) {
            for f in fields.as_array().unwrap() {
                let name = format!("{prefix}{}", f["name"].as_str().unwrap());
                if let Some(sub) = f.get("fields") {
                    walk(sub, &format!("{name}."), out);
                }
                out.push(name);
            }
        }
        let path = format!("{}/schema/{table}.json", env!("CARGO_MANIFEST_DIR"));
        let schema: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut out = Vec::new();
        walk(&schema, "", &mut out);
        out.sort();
        out
    }

    /// Column paths of a serialized row; arrays contribute their first element's fields.
    fn row_columns<R: Row>(row: &R) -> Vec<String> {
        fn walk(v: &Value, prefix: &str, out: &mut Vec<String>) {
            for (k, v) in v.as_object().unwrap() {
                let name = format!("{prefix}{k}");
                if let Some(first) = v.as_array().and_then(|a| a.first()) {
                    walk(first, &format!("{name}."), out);
                }
                out.push(name);
            }
        }
        let mut out = Vec::new();
        walk(&serde_json::to_value(row).unwrap(), "", &mut out);
        out.sort();
        out
    }

    fn check<R: Row>(row: R) {
        assert_eq!(row_columns(&row), schema_columns(R::TABLE), "{}", R::TABLE);
    }

    #[test]
    fn rows_match_the_table_schemas() {
        let d = t("2026-10-22 00:00").date();
        let ts = t("2026-10-22 07:00");
        check(HeartRateRow {
            date: d,
            ts,
            bpm: 60.0,
        });
        check(Spo2Row {
            date: d,
            ts,
            pct: 97.0,
        });
        check(HrvRow {
            date: d,
            ts,
            rmssd_ms: 30.0,
        });
        check(ActivityRow {
            date: d,
            ts,
            steps: None,
            active_kcal: None,
            azm: None,
        });
        check(NightRow {
            date: d,
            start: ts,
            end: ts,
            minutes_asleep: None,
            minutes_awake: None,
            deep_minutes: None,
            light_minutes: None,
            rem_minutes: None,
            awake_minutes: None,
            restless_minutes: None,
            stages: vec![SleepStage {
                start: ts,
                end: ts,
                stage: StageType::Deep,
            }],
            restless: vec![Span { start: ts, end: ts }],
        });
        check(Exercise {
            date: d,
            start: ts,
            end: ts,
            kind: String::new(),
            name: String::new(),
            active_minutes: None,
            avg_hr: None,
            kcal: None,
            azm: None,
        });
        check(DailyMetrics {
            date: d,
            ..Default::default()
        });
    }

    #[test]
    fn rows_serialize_as_the_tables_read_them() {
        let ts = t("2026-10-22 07:00");
        let row = NightRow {
            date: ts.date(),
            start: ts,
            end: ts + Duration::hours(1),
            minutes_asleep: Some(60),
            minutes_awake: None,
            deep_minutes: None,
            light_minutes: None,
            rem_minutes: None,
            awake_minutes: None,
            restless_minutes: Some(0),
            stages: vec![SleepStage {
                start: ts,
                end: ts,
                stage: StageType::Rem,
            }],
            restless: vec![],
        };
        let v = serde_json::to_value(&row).unwrap();
        assert_eq!(v["date"], "2026-10-22");
        assert_eq!(v["end"], "2026-10-22T08:00:00");
        assert_eq!(v["stages"][0]["stage"], "REM");
        assert_eq!(v["minutes_asleep"], json!(60));
    }
}

//! Normalizes parsed API data points (`health.rs`) into typed series, per-day
//! metrics and the rows of the Parquet tables (`tables.rs`).
//!
//! Times are instants in the user's time zone (`Time`): days split at local
//! midnight, and the hour repeated when clocks go back stays two hours.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::engine::{ColumnType, Columns};
use crate::health::{Points, StageType};

/// An instant with the user's UTC offset at that moment; serialized as
/// `2026-10-25T02:30:00+01:00`, stored in UTC (`engine.rs`).
pub type Time = DateTime<chrono::FixedOffset>;

/// Stored by its serde name (`DEEP`, `REM`, ...).
impl ColumnType for StageType {
    fn data_type() -> datafusion::arrow::datatypes::DataType {
        datafusion::arrow::datatypes::DataType::Utf8
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Columns)]
pub struct SleepStage {
    pub start: Time,
    pub end: Time,
    pub stage: StageType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Columns)]
pub struct Span {
    pub start: Time,
    pub end: Time,
}

#[derive(Debug, Clone)]
pub struct Sleep {
    pub start: Time,
    pub end: Time,
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

/// A row of a table: `TABLE` is its name, its fields are the columns.
pub trait Row: Serialize + Columns {
    const TABLE: &'static str;
}

/// Row of `exercises`.
#[derive(Debug, Clone, Serialize, Deserialize, Columns)]
pub struct Exercise {
    /// Local calendar date (partition key)
    pub date: NaiveDate,
    pub start: Time,
    pub end: Time,
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
#[derive(Debug, Clone, Default, Serialize, Deserialize, Columns)]
pub struct DailyMetrics {
    /// Local calendar date (partition key)
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
    pub sleep_start: Option<Time>,
    pub sleep_end: Option<Time>,
    pub exercise_minutes: Option<f64>,
}

impl Row for DailyMetrics {
    const TABLE: &'static str = "daily";
}

#[derive(Debug, Serialize, Columns)]
pub struct HeartRateRow {
    /// Local calendar date (partition key)
    pub date: NaiveDate,
    /// When the reading was taken
    pub ts: Time,
    pub bpm: f64,
}

impl Row for HeartRateRow {
    const TABLE: &'static str = "heart_rate";
}

#[derive(Debug, Serialize, Columns)]
pub struct Spo2Row {
    /// Local calendar date (partition key)
    pub date: NaiveDate,
    /// When the reading was taken
    pub ts: Time,
    pub pct: f64,
}

impl Row for Spo2Row {
    const TABLE: &'static str = "spo2";
}

#[derive(Debug, Serialize, Columns)]
pub struct HrvRow {
    /// Local calendar date (partition key)
    pub date: NaiveDate,
    /// When the reading was taken
    pub ts: Time,
    /// RMSSD over a 5 minute window
    pub rmssd_ms: f64,
}

impl Row for HrvRow {
    const TABLE: &'static str = "hrv";
}

/// Row of `activity`: one minute, merged from three interval series.
#[derive(Debug, Serialize, Columns)]
pub struct ActivityRow {
    /// Local calendar date (partition key)
    pub date: NaiveDate,
    /// Start of the minute
    pub ts: Time,
    pub steps: Option<f64>,
    pub active_kcal: Option<f64>,
    /// Active zone minutes
    pub azm: Option<f64>,
}

impl Row for ActivityRow {
    const TABLE: &'static str = "activity";
}

/// Row of `nights`: the night that ended on `date` (see `Dataset::night_before`).
#[derive(Debug, Clone, Serialize, Deserialize, Columns)]
pub struct NightRow {
    /// Wake-up date
    pub date: NaiveDate,
    pub start: Time,
    pub end: Time,
    pub minutes_asleep: Option<i64>,
    pub minutes_awake: Option<i64>,
    pub deep_minutes: Option<i64>,
    pub light_minutes: Option<i64>,
    pub rem_minutes: Option<i64>,
    pub awake_minutes: Option<i64>,
    /// Short awakenings (restless sleep in Google Health); NULL in rows
    /// loaded before restless sleep was tracked.
    pub restless_minutes: Option<i64>,
    pub stages: Vec<SleepStage>,
    pub restless: Vec<Span>,
}

impl Row for NightRow {
    const TABLE: &'static str = "nights";
}

#[derive(Default)]
pub struct Dataset {
    /// (time, bpm), sorted.
    pub heart_rate: Vec<(Time, f64)>,
    /// Per-minute intervals: (start, value), sorted.
    pub steps: Vec<(Time, f64)>,
    pub active_kcal: Vec<(Time, f64)>,
    pub azm: Vec<(Time, f64)>,
    pub spo2: Vec<(Time, f64)>,
    /// (time, RMSSD ms)
    pub hrv: Vec<(Time, f64)>,
    pub sleep: Vec<Sleep>,
    pub exercise: Vec<Exercise>,
    pub daily: BTreeMap<NaiveDate, DailyMetrics>,
}

/// Longest wake-up between two sleep sessions that still makes them one night.
const MAX_NIGHT_GAP: Duration = Duration::hours(3);

/// Sorted values with a time; points without a value are left out, and of
/// several points at the same instant the first one is kept.
fn series<T>(
    points: &[T],
    tz: Tz,
    point: impl Fn(&T) -> (DateTime<Utc>, Option<f64>),
) -> Vec<(Time, f64)> {
    let mut s: Vec<_> = points
        .iter()
        .filter_map(|p| match point(p) {
            (t, Some(v)) => Some((local(t, tz), v)),
            (_, None) => None,
        })
        .collect();
    s.sort_by_key(|(t, _)| *t);
    s.dedup_by_key(|(t, _)| *t);
    s
}

/// The instant `t` in time zone `tz`.
pub fn local(t: DateTime<Utc>, tz: Tz) -> Time {
    t.with_timezone(&tz).fixed_offset()
}

/// Midnight starting local date `d` in `tz` (the first one, should a zone
/// ever repeat it).
pub fn midnight(d: NaiveDate, tz: Tz) -> Time {
    d.and_time(NaiveTime::MIN)
        .and_local_timezone(tz)
        .earliest()
        // Midnight skipped by a DST change: the day starts an hour later.
        .or_else(|| {
            (d.and_time(NaiveTime::MIN) + Duration::hours(1))
                .and_local_timezone(tz)
                .earliest()
        })
        .expect("a day has a start")
        .fixed_offset()
}

/// `YYYY-MM-DD HH:MM` in Prague, the first of an hour repeated (tests).
#[cfg(test)]
pub fn prague(s: &str) -> Time {
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M")
        .unwrap()
        .and_local_timezone(chrono_tz::Europe::Prague)
        .earliest()
        .unwrap()
        .fixed_offset()
}

/// Whole minutes; summaries report them as integers anyway.
fn minutes(v: Option<f64>) -> Option<i64> {
    v.map(|m| m.round() as i64)
}

impl Dataset {
    /// Normalizes parsed points (typically a few days of them, see
    /// `sync::read_raw`) into typed series and per-day metrics, with local
    /// times and dates in time zone `tz`.
    pub fn from_points(p: Points, tz: Tz) -> Self {
        let mut ds = Dataset {
            heart_rate: series(&p.heart_rate, tz, |x| {
                (x.sample_time.physical_time, x.beats_per_minute)
            }),
            steps: series(&p.steps, tz, |x| (x.interval.start_time, x.count)),
            active_kcal: series(&p.active_energy_burned, tz, |x| {
                (x.interval.start_time, x.kcal)
            }),
            azm: series(&p.active_zone_minutes, tz, |x| {
                (x.interval.start_time, x.active_zone_minutes)
            }),
            spo2: series(&p.oxygen_saturation, tz, |x| {
                (x.sample_time.physical_time, x.percentage)
            }),
            hrv: series(&p.heart_rate_variability, tz, |x| {
                (
                    x.sample_time.physical_time,
                    x.root_mean_square_of_successive_differences_milliseconds,
                )
            }),
            ..Default::default()
        };

        for s in &p.sleep {
            let span = |i: &crate::health::Interval| Span {
                start: local(i.start_time, tz),
                end: local(i.end_time, tz),
            };
            ds.sleep.push(Sleep {
                start: local(s.interval.start_time, tz),
                end: local(s.interval.end_time, tz),
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
            let (start, end) = (
                local(e.interval.start_time, tz),
                local(e.interval.end_time, tz),
            );
            let m = &e.metrics_summary;
            ds.exercise.push(Exercise {
                date: start.date_naive(),
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
        let sum_by_day = |s: &[(Time, f64)]| {
            let mut m: BTreeMap<NaiveDate, f64> = BTreeMap::new();
            for (t, v) in s {
                *m.entry(t.date_naive()).or_default() += v;
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
            let e = hr
                .entry(t.date_naive())
                .or_insert((f64::MAX, f64::MIN, 0.0, 0));
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
            .chain(self.sleep.iter().map(|s| s.end.date_naive()))
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
        let cutoff = NaiveTime::from_hms_opt(14, 0, 0)?;
        let mut parts: Vec<&Sleep> = Vec::new();
        for s in self
            .sleep
            .iter()
            .filter(|s| s.end.date_naive() == d && s.end.time() < cutoff)
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
    let in_range = |t: &Time| (from..=to).contains(&t.date_naive());
    let samples = |s: &[(Time, f64)]| {
        s.iter()
            .filter(|(t, _)| in_range(t))
            .map(|&(t, v)| (t.date_naive(), t, v))
            .collect::<Vec<_>>()
    };

    let mut minutes: BTreeMap<Time, [Option<f64>; 3]> = BTreeMap::new();
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
                date: ts.date_naive(),
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

    use prague as t;

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
        let n = ds.night_before(t("2026-10-22 00:00").date_naive()).unwrap();
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

    /// Column paths of a table's schema, nested records as `a.b`.
    fn schema_columns(table: &str) -> Vec<String> {
        use datafusion::arrow::datatypes::{DataType, Fields};
        fn walk(fields: &Fields, prefix: &str, out: &mut Vec<String>) {
            for f in fields {
                let name = format!("{prefix}{}", f.name());
                let inner = match f.data_type() {
                    DataType::List(item) => item.data_type(),
                    t => t,
                };
                if let DataType::Struct(sub) = inner {
                    walk(sub, &format!("{name}."), out);
                }
                out.push(name);
            }
        }
        let mut out = Vec::new();
        walk(crate::engine::schema(table).fields(), "", &mut out);
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
        let d = t("2026-10-22 00:00").date_naive();
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

    /// Points as the API returns them, in UTC: `offset` is the `utcOffset`
    /// it stamps them with.
    fn points(lines: &[String]) -> Points {
        let mut p = Points::default();
        for l in lines {
            assert!(p.add(serde_json::from_str(l).unwrap()), "{l}");
        }
        p
    }

    fn sample(kind: &str, field: &str, utc: &str, offset: &str, v: f64) -> String {
        json!({ kind: { "sampleTime": { "physicalTime": utc, "utcOffset": offset }, field: v } })
            .to_string()
    }

    fn steps(start: &str, end: &str, n: f64) -> String {
        json!({ "steps": { "interval": { "startTime": start, "startUtcOffset": "0s",
            "endTime": end, "endUtcOffset": "0s" }, "count": n } })
        .to_string()
    }

    #[test]
    fn keeps_both_hours_when_clocks_go_back() {
        // Prague, 2026-10-25: 03:00 CEST becomes 02:00 CET at 01:00 UTC.
        let p = points(&[
            sample(
                "heartRate",
                "beatsPerMinute",
                "2026-10-25T00:30:00Z",
                "7200s",
                50.0,
            ),
            sample(
                "heartRate",
                "beatsPerMinute",
                "2026-10-25T01:30:00Z",
                "3600s",
                55.0,
            ),
            // The API stamps the whole night of HRV with the offset at its end.
            sample(
                "heartRateVariability",
                "rootMeanSquareOfSuccessiveDifferencesMilliseconds",
                "2026-10-24T22:20:00Z",
                "3600s",
                40.0,
            ),
            sample(
                "heartRateVariability",
                "rootMeanSquareOfSuccessiveDifferencesMilliseconds",
                "2026-10-25T00:30:00Z",
                "3600s",
                30.0,
            ),
            sample(
                "heartRateVariability",
                "rootMeanSquareOfSuccessiveDifferencesMilliseconds",
                "2026-10-25T01:30:00Z",
                "3600s",
                20.0,
            ),
            steps("2026-10-25T00:30:00Z", "2026-10-25T00:31:00Z", 10.0),
            steps("2026-10-25T01:30:00Z", "2026-10-25T01:31:00Z", 20.0),
        ]);
        let ds = Dataset::from_points(p, chrono_tz::Europe::Prague);
        let times = |s: &[(Time, f64)]| s.iter().map(|(t, _)| t.to_rfc3339()).collect::<Vec<_>>();
        assert_eq!(
            times(&ds.heart_rate),
            ["2026-10-25T02:30:00+02:00", "2026-10-25T02:30:00+01:00"]
        );
        assert_eq!(
            times(&ds.hrv),
            [
                "2026-10-25T00:20:00+02:00",
                "2026-10-25T02:30:00+02:00",
                "2026-10-25T02:30:00+01:00"
            ]
        );
        let day = &ds.daily[&prague("2026-10-25 00:00").date_naive()];
        assert_eq!(day.steps, Some(30.0));
        assert_eq!(day.hr_avg, Some(52.5));

        let t = table_rows(&ds, day.date, day.date);
        assert_eq!(t.activity.len(), 2);
        assert_eq!(t.heart_rate.len(), 2);
    }

    #[test]
    fn a_night_over_the_clock_change_is_as_long_as_it_was() {
        let at = |s: &str| Time::parse_from_rfc3339(s).unwrap();
        let ds = Dataset {
            sleep: vec![Sleep {
                start: at("2026-10-25T00:00:00+02:00"),
                end: at("2026-10-25T07:00:00+01:00"),
                minutes_asleep: Some(480),
                minutes_awake: None,
                stage_minutes: BTreeMap::new(),
                stages: Vec::new(),
                restless: Vec::new(),
            }],
            ..Default::default()
        };
        let n = ds
            .night_before(prague("2026-10-25 00:00").date_naive())
            .unwrap();
        assert_eq!((n.end - n.start).num_hours(), 8);
    }

    #[test]
    fn days_start_at_local_midnight() {
        let tz = chrono_tz::Europe::Prague;
        let d = prague("2026-10-25 00:00").date_naive();
        assert_eq!(midnight(d, tz).to_rfc3339(), "2026-10-25T00:00:00+02:00");
        let next = midnight(d.succ_opt().unwrap(), tz);
        assert_eq!((next - midnight(d, tz)).num_hours(), 25);
    }

    #[test]
    fn rows_serialize_as_the_tables_read_them() {
        let ts = t("2026-10-22 07:00");
        let row = NightRow {
            date: ts.date_naive(),
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
        assert_eq!(v["end"], "2026-10-22T08:00:00+02:00");
        assert_eq!(v["stages"][0]["stage"], "REM");
        assert_eq!(v["minutes_asleep"], json!(60));
    }
}

//! Payloads of Google Health API data points, as far as normalization uses
//! them. Raw files keep the points exactly as the API returned them; these
//! types are how `model.rs` reads them back, so a renamed or malformed field
//! shows up as a point that failed to parse instead of a silently missing value.

use std::fmt;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize};

/// Data types that are normalized into the tables (`model.rs`).
pub const NORMALIZED: &[&str] = &[
    "heart-rate",
    "heart-rate-variability",
    "oxygen-saturation",
    "steps",
    "active-energy-burned",
    "active-zone-minutes",
    "sleep",
    "exercise",
    "daily-resting-heart-rate",
    "daily-heart-rate-variability",
    "daily-oxygen-saturation",
    "daily-respiratory-rate",
    "daily-sleep-temperature-derivations",
];

/// One raw data point: the payload sits under the camelCase data type name,
/// next to `dataSource` and `name`, which are ignored.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    heart_rate: Option<HeartRate>,
    heart_rate_variability: Option<HeartRateVariability>,
    oxygen_saturation: Option<OxygenSaturation>,
    steps: Option<Steps>,
    active_energy_burned: Option<ActiveEnergy>,
    active_zone_minutes: Option<ActiveZoneMinutes>,
    sleep: Option<Sleep>,
    exercise: Option<Exercise>,
    daily_resting_heart_rate: Option<DailyRestingHeartRate>,
    daily_heart_rate_variability: Option<DailyHeartRateVariability>,
    daily_oxygen_saturation: Option<DailyOxygenSaturation>,
    daily_respiratory_rate: Option<DailyRespiratoryRate>,
    daily_sleep_temperature_derivations: Option<DailySleepTemperature>,
}

/// The parsed points of all `NORMALIZED` types.
#[derive(Default)]
pub struct Points {
    pub heart_rate: Vec<HeartRate>,
    pub heart_rate_variability: Vec<HeartRateVariability>,
    pub oxygen_saturation: Vec<OxygenSaturation>,
    pub steps: Vec<Steps>,
    pub active_energy_burned: Vec<ActiveEnergy>,
    pub active_zone_minutes: Vec<ActiveZoneMinutes>,
    pub sleep: Vec<Sleep>,
    pub exercise: Vec<Exercise>,
    pub daily_resting_heart_rate: Vec<DailyRestingHeartRate>,
    pub daily_heart_rate_variability: Vec<DailyHeartRateVariability>,
    pub daily_oxygen_saturation: Vec<DailyOxygenSaturation>,
    pub daily_respiratory_rate: Vec<DailyRespiratoryRate>,
    pub daily_sleep_temperature_derivations: Vec<DailySleepTemperature>,
}

impl Points {
    /// Adds the payload of `p`; false if it has none of the known ones.
    pub fn add(&mut self, p: Point) -> bool {
        fn put<T>(v: &mut Vec<T>, x: Option<T>, added: &mut bool) {
            if let Some(x) = x {
                v.push(x);
                *added = true;
            }
        }
        let mut added = false;
        put(&mut self.heart_rate, p.heart_rate, &mut added);
        put(
            &mut self.heart_rate_variability,
            p.heart_rate_variability,
            &mut added,
        );
        put(&mut self.oxygen_saturation, p.oxygen_saturation, &mut added);
        put(&mut self.steps, p.steps, &mut added);
        put(
            &mut self.active_energy_burned,
            p.active_energy_burned,
            &mut added,
        );
        put(
            &mut self.active_zone_minutes,
            p.active_zone_minutes,
            &mut added,
        );
        put(&mut self.sleep, p.sleep, &mut added);
        put(&mut self.exercise, p.exercise, &mut added);
        put(
            &mut self.daily_resting_heart_rate,
            p.daily_resting_heart_rate,
            &mut added,
        );
        put(
            &mut self.daily_heart_rate_variability,
            p.daily_heart_rate_variability,
            &mut added,
        );
        put(
            &mut self.daily_oxygen_saturation,
            p.daily_oxygen_saturation,
            &mut added,
        );
        put(
            &mut self.daily_respiratory_rate,
            p.daily_respiratory_rate,
            &mut added,
        );
        put(
            &mut self.daily_sleep_temperature_derivations,
            p.daily_sleep_temperature_derivations,
            &mut added,
        );
        added
    }
}

// ------------------------------------------------------------- time

/// A protobuf `Duration` in JSON: decimal seconds with an `s` suffix.
fn seconds<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
    let s = String::deserialize(d)?;
    let secs: f64 = s
        .strip_suffix('s')
        .and_then(|n| n.parse().ok())
        .filter(|n: &f64| n.is_finite())
        .ok_or_else(|| de::Error::custom(format!("expected a duration like \"60s\", got {s:?}")))?;
    Ok(Duration::milliseconds((secs * 1000.0).round() as i64))
}

fn opt_seconds<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
    seconds(d).map(Some)
}

/// When a sample was taken. Its `utcOffset` is ignored: local time comes
/// from the user's time zone (`model.rs`), because the API stamps a whole
/// night of HRV with the offset at its end, wrong by an hour on the nights
/// clocks change.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleTime {
    pub physical_time: DateTime<Utc>,
}

/// A time range; also the shape of sleep stages and short awakenings.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Interval {
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
}

/// `{"year": 2026, "month": 10, "day": 24}`
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CivilDate(pub NaiveDate);

impl<'de> Deserialize<'de> for CivilDate {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Ymd {
            year: i32,
            month: u32,
            day: u32,
        }
        let Ymd { year, month, day } = Ymd::deserialize(d)?;
        NaiveDate::from_ymd_opt(year, month, day)
            .map(CivilDate)
            .ok_or_else(|| de::Error::custom(format!("invalid date {year}-{month}-{day}")))
    }
}

// ----------------------------------------------------------- numbers

/// A measurement: a JSON number or a numeric string (int64 fields arrive as
/// strings). `"NaN"` and infinities mean no value; anything else is an error.
fn num<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    struct V;
    impl Visitor<'_> for V {
        type Value = Option<f64>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a number or a numeric string")
        }
        fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> {
            Ok(v.is_finite().then_some(v))
        }
        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Some(v as f64))
        }
        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
            Ok(Some(v as f64))
        }
        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            let v: f64 = s
                .parse()
                .map_err(|_| E::custom(format!("not a number: {s:?}")))?;
            Ok(v.is_finite().then_some(v))
        }
    }
    d.deserialize_any(V)
}

// ---------------------------------------------------------- intraday

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartRate {
    pub sample_time: SampleTime,
    #[serde(deserialize_with = "num")]
    pub beats_per_minute: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartRateVariability {
    pub sample_time: SampleTime,
    #[serde(deserialize_with = "num")]
    pub root_mean_square_of_successive_differences_milliseconds: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OxygenSaturation {
    pub sample_time: SampleTime,
    #[serde(deserialize_with = "num")]
    pub percentage: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct Steps {
    pub interval: Interval,
    #[serde(deserialize_with = "num")]
    pub count: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct ActiveEnergy {
    pub interval: Interval,
    #[serde(deserialize_with = "num")]
    pub kcal: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveZoneMinutes {
    pub interval: Interval,
    #[serde(deserialize_with = "num")]
    pub active_zone_minutes: Option<f64>,
}

// ---------------------------------------------------------- sessions

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StageType {
    Awake,
    Light,
    Deep,
    Rem,
    /// Sleep without stages (older devices, or too short for stages).
    Asleep,
    Restless,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sleep {
    pub interval: Interval,
    #[serde(default)]
    pub stages: Vec<Stage>,
    #[serde(default)]
    pub summary: SleepSummary,
    /// Short wake-ups inside light sleep, shown as restless sleep.
    #[serde(default)]
    pub short_awakenings: Vec<Interval>,
}

#[derive(Debug, Deserialize)]
pub struct Stage {
    #[serde(flatten)]
    pub interval: Interval,
    #[serde(rename = "type")]
    pub stage: StageType,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SleepSummary {
    #[serde(default)]
    pub stages_summary: Vec<StageSummary>,
    #[serde(default, deserialize_with = "num")]
    pub minutes_asleep: Option<f64>,
    #[serde(default, deserialize_with = "num")]
    pub minutes_awake: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct StageSummary {
    #[serde(rename = "type")]
    pub stage: StageType,
    #[serde(deserialize_with = "num")]
    pub minutes: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Exercise {
    pub interval: Interval,
    pub exercise_type: Option<String>,
    #[serde(default)]
    pub display_name: String,
    #[serde(default, deserialize_with = "opt_seconds")]
    pub active_duration: Option<Duration>,
    #[serde(default)]
    pub metrics_summary: ExerciseMetrics,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExerciseMetrics {
    #[serde(default, deserialize_with = "num")]
    pub average_heart_rate_beats_per_minute: Option<f64>,
    #[serde(default, deserialize_with = "num")]
    pub calories_kcal: Option<f64>,
    #[serde(default, deserialize_with = "num")]
    pub active_zone_minutes: Option<f64>,
}

// ------------------------------------------------------------- daily

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyRestingHeartRate {
    pub date: CivilDate,
    #[serde(default, deserialize_with = "num")]
    pub beats_per_minute: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyHeartRateVariability {
    pub date: CivilDate,
    #[serde(default, deserialize_with = "num")]
    pub average_heart_rate_variability_milliseconds: Option<f64>,
    #[serde(default, deserialize_with = "num")]
    pub deep_sleep_root_mean_square_of_successive_differences_milliseconds: Option<f64>,
    #[serde(default, deserialize_with = "num")]
    pub non_rem_heart_rate_beats_per_minute: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyOxygenSaturation {
    pub date: CivilDate,
    #[serde(default, deserialize_with = "num")]
    pub average_percentage: Option<f64>,
    #[serde(default, deserialize_with = "num")]
    pub lower_bound_percentage: Option<f64>,
    #[serde(default, deserialize_with = "num")]
    pub upper_bound_percentage: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyRespiratoryRate {
    pub date: CivilDate,
    #[serde(default, deserialize_with = "num")]
    pub breaths_per_minute: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailySleepTemperature {
    pub date: CivilDate,
    #[serde(default, deserialize_with = "num")]
    pub nightly_temperature_celsius: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Point {
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn parses_a_sample() {
        let p = parse(
            r#"{"heartRate": {"beatsPerMinute": "61", "sampleTime": {"physicalTime": "2026-10-23T17:55:56Z",
                "utcOffset": "7200s", "civilTime": {}}}, "dataSource": {"device": {}}, "name": "x"}"#,
        );
        let hr = p.heart_rate.unwrap();
        assert_eq!(hr.beats_per_minute, Some(61.0));
        assert_eq!(
            hr.sample_time.physical_time.to_rfc3339(),
            "2026-10-23T17:55:56+00:00"
        );
    }

    #[test]
    fn numbers_may_be_strings_or_nan_but_not_garbage() {
        let spo2 = |v: &str| {
            serde_json::from_str::<Point>(&format!(
                r#"{{"oxygenSaturation": {{"percentage": {v},
                    "sampleTime": {{"physicalTime": "2026-10-23T05:00:00Z", "utcOffset": "0s"}}}}}}"#
            ))
            .map(|p| p.oxygen_saturation.unwrap().percentage)
        };
        assert_eq!(spo2("96.6").unwrap(), Some(96.6));
        assert_eq!(spo2(r#""97""#).unwrap(), Some(97.0));
        assert_eq!(spo2(r#""NaN""#).unwrap(), None);
        assert!(spo2(r#""high""#).is_err());
        assert!(spo2("null").is_err());
    }

    #[test]
    fn missing_fields_are_errors() {
        // A renamed field must not turn into a missing value.
        assert!(
            serde_json::from_str::<Point>(
                r#"{"steps": {"stepCount": "5", "interval": {"startTime": "2026-10-23T05:00:00Z",
                    "startUtcOffset": "0s", "endTime": "2026-10-23T05:01:00Z", "endUtcOffset": "0s"}}}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<Point>(r#"{"dailyRestingHeartRate": {"beatsPerMinute": 58}}"#)
                .is_err()
        );
    }

    #[test]
    fn parses_sleep() {
        let p = parse(
            r#"{"sleep": {"interval": {"startTime": "2026-10-23T21:18:00Z", "startUtcOffset": "7200s",
                "endTime": "2026-10-24T05:22:00Z", "endUtcOffset": "7200s"},
              "metadata": {"mainSleep": true},
              "stages": [{"startTime": "2026-10-23T21:18:00Z", "startUtcOffset": "7200s",
                "endTime": "2026-10-23T21:26:00Z", "endUtcOffset": "7200s", "type": "AWAKE"},
                {"startTime": "2026-10-23T21:26:00Z", "startUtcOffset": "7200s",
                "endTime": "2026-10-23T21:40:00Z", "endUtcOffset": "7200s", "type": "SOMETHING_NEW"}],
              "summary": {"minutesAsleep": "468", "stagesSummary": [{"type": "DEEP", "minutes": "60"}]}},
             "dataPointName": "users/1"}"#,
        );
        let s = p.sleep.unwrap();
        assert_eq!(
            s.interval.end_time.to_rfc3339(),
            "2026-10-24T05:22:00+00:00"
        );
        assert_eq!(s.stages[0].stage, StageType::Awake);
        assert_eq!(s.stages[1].stage, StageType::Unknown);
        assert_eq!(s.summary.minutes_asleep, Some(468.0));
        assert_eq!(s.summary.minutes_awake, None);
        assert_eq!(s.summary.stages_summary[0].stage, StageType::Deep);
        assert!(s.short_awakenings.is_empty());
    }

    #[test]
    fn parses_exercise_and_daily_dates() {
        let p = parse(
            r#"{"exercise": {"interval": {"startTime": "2026-10-23T09:45:26.800Z", "startUtcOffset": "7200s",
                "endTime": "2026-10-23T10:37:41.800Z", "endUtcOffset": "7200s"},
              "activeDuration": "3135s", "displayName": "Kolo", "exerciseType": "BIKING",
              "metricsSummary": {"caloriesKcal": 536, "averageHeartRateBeatsPerMinute": "137"}}}"#,
        );
        let e = p.exercise.unwrap();
        assert_eq!(e.active_duration, Some(Duration::seconds(3135)));
        assert_eq!(e.metrics_summary.calories_kcal, Some(536.0));
        assert_eq!(e.metrics_summary.active_zone_minutes, None);
        let d = parse(
            r#"{"dailyRespiratoryRate": {"breathsPerMinute": 12.4, "date": {"year": 2026, "month": 10, "day": 24}}}"#,
        );
        assert_eq!(
            d.daily_respiratory_rate.unwrap().date.0,
            NaiveDate::from_ymd_opt(2026, 10, 24).unwrap()
        );
        assert!(
            serde_json::from_str::<Point>(
                r#"{"dailyRespiratoryRate": {"date": {"year": 2026, "month": 13, "day": 1}}}"#
            )
            .is_err()
        );
    }
}

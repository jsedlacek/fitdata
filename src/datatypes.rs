//! Catalog of Google Health API v4 data types (from the `DataPoint` union in the
//! discovery document) and how each one is filtered by time.

use chrono::NaiveDate;

/// How a data type is anchored in time; determines the filter field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Point-in-time observation, filtered by `sample_time.civil_time`.
    Sample,
    /// Time range, filtered by `interval.civil_start_time`.
    Interval,
    /// One value per civil day, filtered by `date`.
    Daily,
    /// Session (exercise, sleep...), filtered by `interval.civil_start_time`.
    Session,
    /// Sleep sessions can only be filtered by their end time.
    SleepSession,
    /// ECG supports only a `start_time >=` filter, so it is fetched in one go.
    StartOnly,
}

/// Size of one fetch window; also the granularity of the stored files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    /// Intraday data: one file per civil day.
    Day,
    /// Sparse data: one file per calendar month.
    Month,
    /// Everything in a single file.
    All,
}

#[derive(Debug, Clone, Copy)]
pub struct DataType {
    pub id: &'static str,
    pub kind: Kind,
    pub window: Window,
    /// Only readable via `dataPoints:reconcile`, not `list` (e.g. sleep, floors).
    pub reconcile: bool,
}

const fn dt(id: &'static str, kind: Kind, window: Window) -> DataType {
    DataType {
        id,
        kind,
        window,
        reconcile: false,
    }
}

const fn reconciled(id: &'static str, kind: Kind, window: Window) -> DataType {
    DataType {
        id,
        kind,
        window,
        reconcile: true,
    }
}

use Kind::*;
use Window::*;

pub const ALL: &[DataType] = &[
    // Intraday samples and intervals.
    dt("heart-rate", Sample, Day),
    dt("heart-rate-variability", Sample, Day),
    dt("oxygen-saturation", Sample, Day),
    dt("core-body-temperature", Sample, Day),
    dt("steps", Interval, Day),
    dt("distance", Interval, Day),
    reconciled("floors", Interval, Day),
    dt("altitude", Interval, Day),
    dt("active-energy-burned", Interval, Day),
    dt("basal-energy-burned", Interval, Day),
    dt("active-minutes", Interval, Day),
    dt("active-zone-minutes", Interval, Day),
    dt("time-in-heart-rate-zone", Interval, Day),
    dt("sedentary-period", Interval, Day),
    dt("activity-level", Interval, Day),
    dt("swim-lengths-data", Interval, Day),
    // Sparse samples.
    dt("respiratory-rate-sleep-summary", Sample, Month),
    dt("vo2-max", Sample, Month),
    dt("run-vo2-max", Sample, Month),
    dt("weight", Sample, Month),
    dt("height", Sample, Month),
    dt("body-fat", Sample, Month),
    dt("blood-glucose", Sample, Month),
    dt("moods", Sample, Month),
    dt("symptoms", Sample, Month),
    dt("ovulation-test", Sample, Month),
    dt("menstrual-period", Interval, Month),
    // Daily summaries.
    dt("daily-heart-rate-variability", Daily, Month),
    dt("daily-heart-rate-zones", Daily, Month),
    dt("daily-oxygen-saturation", Daily, Month),
    dt("daily-respiratory-rate", Daily, Month),
    dt("daily-resting-heart-rate", Daily, Month),
    dt("daily-sleep-temperature-derivations", Daily, Month),
    dt("daily-vo2-max", Daily, Month),
    // Sessions.
    reconciled("sleep", SleepSession, Month),
    dt("exercise", Session, Month),
    dt("hydration-log", Session, Month),
    dt("nutrition-log", Session, Month),
    dt("irregular-rhythm-notification", Session, Month),
    dt("electrocardiogram", StartOnly, All),
];

pub fn find(id: &str) -> Option<&'static DataType> {
    ALL.iter().find(|t| t.id == id)
}

impl DataType {
    /// AIP-160 filter selecting points in the civil date range `[from, to)`.
    /// `None` for `StartOnly` types, which are fetched without a filter.
    pub fn filter(&self, from: NaiveDate, to: NaiveDate) -> Option<String> {
        let f = self.id.replace('-', "_");
        let range = |field: String| format!(r#"{field} >= "{from}" AND {field} < "{to}""#);
        match self.kind {
            Sample => Some(range(format!("{f}.sample_time.civil_time"))),
            Interval | Session => Some(range(format!("{f}.interval.civil_start_time"))),
            Daily => Some(range(format!("{f}.date"))),
            SleepSession => Some(range(format!("{f}.interval.civil_end_time"))),
            StartOnly => None,
        }
    }

    /// Sleep and exercise only allow small pages.
    pub fn page_size(&self) -> u32 {
        match self.id {
            "sleep" | "exercise" => 25,
            _ => 10_000,
        }
    }
}

/// OAuth scopes needed to read every data type.
pub const READ_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/googlehealth.activity_and_fitness.readonly",
    "https://www.googleapis.com/auth/googlehealth.health_metrics_and_measurements.readonly",
    "https://www.googleapis.com/auth/googlehealth.sleep.readonly",
    "https://www.googleapis.com/auth/googlehealth.profile.readonly",
    "https://www.googleapis.com/auth/googlehealth.settings.readonly",
    "https://www.googleapis.com/auth/googlehealth.irn.readonly",
    "https://www.googleapis.com/auth/googlehealth.ecg.readonly",
    "https://www.googleapis.com/auth/googlehealth.mindfulness.readonly",
    "https://www.googleapis.com/auth/googlehealth.logged_symptoms.readonly",
    "https://www.googleapis.com/auth/googlehealth.reproductive_health.readonly",
    "https://www.googleapis.com/auth/googlehealth.location.readonly",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_use_snake_case_fields() {
        let d = |s| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        assert_eq!(
            find("heart-rate")
                .unwrap()
                .filter(d("2026-10-01"), d("2026-10-02"))
                .unwrap(),
            r#"heart_rate.sample_time.civil_time >= "2026-10-01" AND heart_rate.sample_time.civil_time < "2026-10-02""#
        );
        assert_eq!(
            find("daily-heart-rate-variability")
                .unwrap()
                .filter(d("2026-10-01"), d("2026-11-01"))
                .unwrap(),
            r#"daily_heart_rate_variability.date >= "2026-10-01" AND daily_heart_rate_variability.date < "2026-11-01""#
        );
        assert!(
            find("electrocardiogram")
                .unwrap()
                .filter(d("2026-10-01"), d("2026-10-02"))
                .is_none()
        );
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = ALL.iter().map(|t| t.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), ALL.len());
    }
}

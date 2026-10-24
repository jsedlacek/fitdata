//! Fetch windows of data points and persist them as raw NDJSON.
//!
//! Layout: `raw/{data-type}/{YYYY-MM-DD | YYYY-MM | all}.jsonl`, one API data
//! point per line, exactly as returned by the API.

use anyhow::Result;
use chrono::{Datelike, Months, NaiveDate};
use futures::{StreamExt, TryStreamExt, stream};
use serde_json::{Value, json};
use tracing::Instrument;

use crate::api::{ApiError, HealthClient};
use crate::datatypes::{self, DataType, Window};
use crate::health::{self, Point, Points};
use crate::store::Store;

/// Types fetched concurrently; requests within a type are sequential.
const CONCURRENCY: usize = 4;

#[derive(Debug, Clone)]
struct Slot {
    key: String,
    from: NaiveDate,
    to: NaiveDate,
}

fn month_start(d: NaiveDate) -> NaiveDate {
    d.with_day(1).unwrap()
}

fn slot_for(dt: &DataType, d: NaiveDate) -> Slot {
    match dt.window {
        Window::Day => Slot {
            key: d.to_string(),
            from: d,
            to: d.succ_opt().unwrap(),
        },
        Window::Month => {
            let from = month_start(d);
            Slot {
                key: from.format("%Y-%m").to_string(),
                from,
                to: from + Months::new(1),
            }
        }
        Window::All => Slot {
            key: "all".into(),
            from: NaiveDate::MIN,
            to: NaiveDate::MAX,
        },
    }
}

/// Distinct slots covering `[from, to)`.
fn slots(dt: &DataType, from: NaiveDate, to: NaiveDate) -> Vec<Slot> {
    let mut out: Vec<Slot> = Vec::new();
    let mut d = from;
    while d < to {
        let s = slot_for(dt, d);
        d = s.to.max(d.succ_opt().unwrap());
        out.push(s);
        if dt.window == Window::All {
            break;
        }
    }
    out
}

fn object_path(dt: &DataType, slot: &Slot) -> String {
    format!("raw/{}/{}.jsonl", dt.id, slot.key)
}

/// Reads and parses the stored raw points of the normalized data types for
/// slots overlapping the civil date range `[from, to)`.
///
/// Points that do not parse (the API changed a field, say) are left out and
/// reported as an error per data type, so the rest of the day still loads.
pub async fn read_raw(store: &Store, from: NaiveDate, to: NaiveDate) -> Result<Points> {
    let reads = health::NORMALIZED.iter().map(|id| async move {
        let dt = datatypes::find(id).expect("normalized types are in the catalog");
        let mut files = Vec::new();
        for slot in slots(dt, from, to) {
            if let Some(data) = store.get(&object_path(dt, &slot)).await? {
                files.push(data);
            }
        }
        Ok::<_, anyhow::Error>((*id, files))
    });
    // A few types at a time: a week of heart rate alone is ~300k points.
    let read: Vec<(&str, Vec<Vec<u8>>)> = stream::iter(reads)
        .buffer_unordered(4)
        .try_collect()
        .await?;

    let mut points = Points::default();
    for (id, files) in read {
        let (mut bad, mut example) = (0, None);
        for data in files {
            for line in std::str::from_utf8(&data)?
                .lines()
                .filter(|l| !l.is_empty())
            {
                let parsed = serde_json::from_str::<Point>(line)
                    .map_err(|e| e.to_string())
                    .and_then(|p| {
                        points
                            .add(p)
                            .then_some(())
                            .ok_or_else(|| "no known payload".to_string())
                    });
                if let Err(e) = parsed {
                    bad += 1;
                    example.get_or_insert(e);
                }
            }
        }
        if let Some(e) = example {
            tracing::error!(
                data_type = id,
                "{bad} stored points could not be read, e.g.: {e}"
            );
        }
    }
    Ok(points)
}

/// Fetches one slot and writes it; returns the number of points. A slot
/// without points has no file, so data deleted in the app goes away here too.
async fn fetch_slot(
    client: &HealthClient,
    store: &Store,
    dt: &DataType,
    slot: &Slot,
) -> Result<usize> {
    let filter = dt.filter(slot.from, slot.to);
    let points = client.list_all(dt, filter.as_deref()).await?;
    let path = object_path(dt, slot);
    if points.is_empty() {
        store.delete(&path).await?;
    } else {
        store.put_jsonl(&path, &points).await?;
    }
    tracing::info!("{}: {} points", slot.key, points.len());
    Ok(points.len())
}

/// Groups the requests and logs of one data type.
fn type_span(dt: &DataType) -> tracing::Span {
    tracing::info_span!(
        "fetch",
        otel.name = format!("fetch {}", dt.id),
        data_type = dt.id
    )
}

/// Treats "no access / unsupported for this account" as no data instead of
/// failing the run. A 400 is not among them: it means a bad request (e.g. a
/// filter the API no longer accepts), which must not pass silently.
fn is_unavailable(e: &anyhow::Error) -> bool {
    e.downcast_ref::<ApiError>()
        .is_some_and(|e| matches!(e.status, 403 | 404))
}

/// What `fetch_range` did.
#[derive(Debug, Default)]
pub struct Fetched {
    pub points: usize,
    /// Data types skipped because they are not readable (see `is_unavailable`).
    pub skipped: Vec<&'static str>,
}

/// Re-fetches every slot overlapping `[from, to)`, overwriting stored files.
/// Used by the daily job with a trailing window, since devices sync late.
pub async fn fetch_range(
    client: &HealthClient,
    store: &Store,
    types: &[&'static DataType],
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Fetched> {
    let per_type: Vec<(&'static str, usize, bool)> = stream::iter(types.iter().copied())
        .map(|dt| {
            async move {
                let mut n = 0;
                for slot in slots(dt, from, to) {
                    match fetch_slot(client, store, dt, &slot).await {
                        Ok(c) => n += c,
                        Err(e) if is_unavailable(&e) => {
                            tracing::warn!("skipped ({e})");
                            return Ok((dt.id, n, true));
                        }
                        Err(e) => return Err(e.context(format!("{} {}", dt.id, slot.key))),
                    }
                }
                Ok::<_, anyhow::Error>((dt.id, n, false))
            }
            .instrument(type_span(dt))
        })
        .buffer_unordered(CONCURRENCY)
        .try_collect()
        .await?;
    let mut out = Fetched::default();
    for (id, n, skipped) in per_type {
        out.points += n;
        if skipped {
            out.skipped.push(id);
        }
    }
    out.skipped.sort_unstable();
    Ok(out)
}

/// Walks back month by month from the newest data point of each type until
/// `max_empty_months` consecutive months have no data (or `since` is reached).
/// Already stored slots are skipped, so an interrupted backfill can resume.
pub async fn backfill(
    client: &HealthClient,
    store: &Store,
    types: &[&'static DataType],
    since: Option<NaiveDate>,
    max_empty_months: u32,
) -> Result<usize> {
    let totals: Vec<usize> = stream::iter(types.iter().copied())
        .map(|dt| {
            async move {
                match backfill_type(client, store, dt, since, max_empty_months).await {
                    Ok(n) => Ok(n),
                    Err(e) if is_unavailable(&e) => {
                        tracing::warn!("skipped ({e})");
                        Ok(0)
                    }
                    Err(e) => Err(e.context(dt.id)),
                }
            }
            .instrument(type_span(dt))
        })
        .buffer_unordered(CONCURRENCY)
        .try_collect()
        .await?;
    Ok(totals.into_iter().sum())
}

async fn backfill_type(
    client: &HealthClient,
    store: &Store,
    dt: &'static DataType,
    since: Option<NaiveDate>,
    max_empty_months: u32,
) -> Result<usize> {
    let Some(newest) = client.newest(dt, None).await? else {
        tracing::info!("no data");
        return Ok(0);
    };
    if dt.window == Window::All {
        return fetch_slot(client, store, dt, &slot_for(dt, NaiveDate::MIN)).await;
    }
    let newest_date = point_date(&newest).unwrap_or_else(|| chrono::Local::now().date_naive());
    let floor = since.unwrap_or(NaiveDate::MIN);

    let mut total = 0;
    let mut empty_streak = 0;
    let mut month = month_start(newest_date);
    while month + Months::new(1) > floor && empty_streak < max_empty_months {
        let next = month + Months::new(1);
        let from = month.max(floor);
        // One cheap request tells whether the month has any data at all.
        let any = client
            .newest(dt, dt.filter(from, next).as_deref())
            .await?
            .is_some();
        if any {
            empty_streak = 0;
            for slot in slots(dt, from, next.min(newest_date.succ_opt().unwrap())) {
                if !store.exists(&object_path(dt, &slot)).await? {
                    total += fetch_slot(client, store, dt, &slot).await?;
                }
            }
        } else {
            empty_streak += 1;
            tracing::info!("{}: empty", month.format("%Y-%m"));
        }
        month = month - Months::new(1);
    }
    Ok(total)
}

/// The data object inside a point, e.g. `point["heartRate"]`.
fn payload(point: &Value) -> Option<&Value> {
    point
        .as_object()?
        .iter()
        .find(|(k, v)| *k != "dataSource" && *k != "name" && v.is_object())
        .map(|(_, v)| v)
}

/// Best-effort timestamp of a point for display: sample time, interval start or date.
pub fn point_time(point: &Value) -> Option<String> {
    let p = payload(point)?;
    if let Some(t) = p["sampleTime"]["physicalTime"].as_str() {
        return Some(t.to_string());
    }
    if let Some(t) = p["interval"]["startTime"].as_str() {
        return Some(t.to_string());
    }
    let d = &p["date"];
    Some(format!(
        "{:04}-{:02}-{:02}",
        d["year"].as_i64()?,
        d["month"].as_i64()?,
        d["day"].as_i64()?
    ))
}

fn point_date(point: &Value) -> Option<NaiveDate> {
    let t = point_time(point)?;
    NaiveDate::parse_from_str(t.get(..10)?, "%Y-%m-%d").ok()
}

/// For each type: whether we can read it and when its newest point is.
pub async fn probe(client: &HealthClient, types: &[&'static DataType]) -> Vec<Value> {
    stream::iter(types.iter().copied())
        .map(|dt| async move {
            let (status, newest, sample) = match client.newest(dt, None).await {
                Ok(Some(p)) => ("data", point_time(&p), Some(p)),
                Ok(None) => ("empty", None, None),
                Err(e) => (
                    if is_unavailable(&e) {
                        "unavailable"
                    } else {
                        "error"
                    },
                    Some(e.to_string()),
                    None,
                ),
            };
            json!({ "dataType": dt.id, "status": status, "newest": newest, "sample": sample })
        })
        .buffered(8)
        .collect()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datatypes::find;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn day_slots_cover_range() {
        let s = slots(
            find("heart-rate").unwrap(),
            d("2026-10-30"),
            d("2026-11-02"),
        );
        let keys: Vec<_> = s.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["2026-10-30", "2026-10-31", "2026-11-01"]);
    }

    #[test]
    fn month_slots_are_whole_months() {
        let s = slots(find("sleep").unwrap(), d("2026-09-20"), d("2026-10-05"));
        let keys: Vec<_> = s.iter().map(|s| (s.key.as_str(), s.from, s.to)).collect();
        assert_eq!(
            keys,
            [
                ("2026-09", d("2026-09-01"), d("2026-10-01")),
                ("2026-10", d("2026-10-01"), d("2026-11-01"))
            ]
        );
    }

    #[test]
    fn point_time_handles_all_shapes() {
        let sample = json!({"heartRate": {"sampleTime": {"physicalTime": "2026-10-22T10:00:00Z"}, "beatsPerMinute": "60"}, "dataSource": {}});
        let daily =
            json!({"dailyRestingHeartRate": {"date": {"year": 2026, "month": 10, "day": 2}}});
        assert_eq!(point_time(&sample).unwrap(), "2026-10-22T10:00:00Z");
        assert_eq!(point_date(&daily).unwrap(), d("2026-10-02"));
    }
}

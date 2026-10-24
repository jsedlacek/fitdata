//! Fetch windows of data points and persist them as raw NDJSON.
//!
//! Layout: `raw/{data-type}/{YYYY-MM-DD | YYYY-MM | all}.jsonl`, one API data
//! point per line, exactly as returned by the API.

use anyhow::Result;
use chrono::{Datelike, Months, NaiveDate};
use futures::{StreamExt, TryStreamExt, stream};
use serde_json::{Value, json};

use crate::api::{ApiError, HealthClient};
use crate::datatypes::{DataType, Window};
use crate::model::Raw;
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

/// Reads the stored raw points of all data types for slots overlapping the
/// civil date range `[from, to)`.
pub async fn read_raw(store: &Store, from: NaiveDate, to: NaiveDate) -> Result<Raw> {
    let reads = crate::datatypes::ALL.iter().map(|dt| async move {
        let mut points = Vec::new();
        for slot in slots(dt, from, to) {
            points.extend(
                store
                    .get_jsonl(&object_path(dt, &slot))
                    .await?
                    .into_iter()
                    .filter_map(slim),
            );
        }
        Ok::<_, anyhow::Error>((dt.id.to_string(), points))
    });
    // A few types at a time: a week of heart rate alone is ~300k points.
    stream::iter(reads).buffer_unordered(4).try_collect().await
}

/// Keeps only what normalization uses: the payload object (e.g. the value of
/// `"heartRate"`) without the civil-time copies of each timestamp. Cuts the
/// memory of a week of raw data several times over.
fn slim(mut point: Value) -> Option<Value> {
    let key = point
        .as_object()?
        .iter()
        .find(|(k, v)| *k != "dataSource" && *k != "name" && v.is_object())
        .map(|(k, _)| k.clone())?;
    let mut p = point[&key].take();
    if let Some(t) = p.get_mut("sampleTime").and_then(Value::as_object_mut) {
        t.remove("civilTime");
    }
    if let Some(i) = p.get_mut("interval").and_then(Value::as_object_mut) {
        i.remove("civilStartTime");
        i.remove("civilEndTime");
    }
    Some(p)
}

/// Fetches one slot and writes it; returns the number of points.
async fn fetch_slot(
    client: &HealthClient,
    store: &Store,
    dt: &DataType,
    slot: &Slot,
) -> Result<usize> {
    let filter = dt.filter(slot.from, slot.to);
    let points = client.list_all(dt, filter.as_deref()).await?;
    if !points.is_empty() {
        store.put_jsonl(&object_path(dt, slot), &points).await?;
    }
    tracing::info!("{:<36} {:<10} {:>6} points", dt.id, slot.key, points.len());
    Ok(points.len())
}

/// Treats "no access / unsupported for this account" as no data instead of failing the run.
fn is_unavailable(e: &anyhow::Error) -> bool {
    e.downcast_ref::<ApiError>()
        .is_some_and(|e| matches!(e.status, 400 | 403 | 404))
}

/// Re-fetches every slot overlapping `[from, to)`, overwriting stored files.
/// Used by the daily job with a trailing window, since devices sync late.
pub async fn fetch_range(
    client: &HealthClient,
    store: &Store,
    types: &[&'static DataType],
    from: NaiveDate,
    to: NaiveDate,
) -> Result<usize> {
    let totals: Vec<usize> = stream::iter(types.iter().copied())
        .map(|dt| async move {
            let mut n = 0;
            for slot in slots(dt, from, to) {
                match fetch_slot(client, store, dt, &slot).await {
                    Ok(c) => n += c,
                    Err(e) if is_unavailable(&e) => {
                        tracing::warn!("{}: skipped ({e})", dt.id);
                        break;
                    }
                    Err(e) => return Err(e.context(format!("{} {}", dt.id, slot.key))),
                }
            }
            Ok::<_, anyhow::Error>(n)
        })
        .buffer_unordered(CONCURRENCY)
        .try_collect()
        .await?;
    Ok(totals.into_iter().sum())
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
        .map(|dt| async move {
            match backfill_type(client, store, dt, since, max_empty_months).await {
                Ok(n) => Ok(n),
                Err(e) if is_unavailable(&e) => {
                    tracing::warn!("{}: skipped ({e})", dt.id);
                    Ok(0)
                }
                Err(e) => Err(e.context(dt.id)),
            }
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
        tracing::info!("{:<36} no data", dt.id);
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
            tracing::info!("{:<36} {:<10} empty", dt.id, month.format("%Y-%m"));
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
    fn slim_keeps_only_the_payload() {
        let p = json!({
            "heartRate": {
                "beatsPerMinute": "61",
                "sampleTime": {"physicalTime": "2026-10-23T17:55:56Z", "utcOffset": "7200s", "civilTime": {"date": {}}}
            },
            "dataSource": {"device": {"displayName": "Fitbit Air"}}
        });
        assert_eq!(
            slim(p).unwrap(),
            json!({"beatsPerMinute": "61", "sampleTime": {"physicalTime": "2026-10-23T17:55:56Z", "utcOffset": "7200s"}})
        );
        let steps = json!({"steps": {"count": "5", "interval": {"startTime": "a", "civilStartTime": {}, "civilEndTime": {}}}});
        assert_eq!(
            slim(steps).unwrap(),
            json!({"count": "5", "interval": {"startTime": "a"}})
        );
        assert!(slim(json!({"dataSource": {}})).is_none());
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

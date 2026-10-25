#!/usr/bin/env python3
"""Writes 90 days of made-up Google Health API data points for a demo user, for
the screenshots on the home page (frontend/src/home/), so no real data is shown.

    python3 scripts/demo_data.py /tmp/demo 4815162342108
    # users/4815162342108.json with a dummy token, time_zone "Europe/Prague" and
    # prefs {"units": "metric", "weekStart": "monday", "clock": "24h"}, then:
    cargo run -- --storage /tmp/demo load --user 4815162342108 --from 2026-07-28
    cargo run -- --storage /tmp/demo serve --dev-user 4815162342108

Take the screenshots with the browser locale en-GB and time zone Europe/Prague.
"""
import json, math, os, random, sys
from datetime import date, datetime, timedelta, timezone
from zoneinfo import ZoneInfo

random.seed(7)
TZ = ZoneInfo("Europe/Prague")
root, uid = sys.argv[1], sys.argv[2]
raw = os.path.join(root, "u", uid, "raw")
END = date(2026, 10, 25)
DAYS = 90
NOW = datetime(2026, 10, 25, 16, 40, tzinfo=TZ)
# A late night out (the home page's chat is about it): short sleep, little
# deep sleep, a high heart rate and a low HRV through the night.
PARTY = date(2026, 10, 18)

files = {}
def put(dtype, key, point):
    files.setdefault((dtype, key), []).append(point)

def iso(t): return t.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
def off(t): return f"{int(t.utcoffset().total_seconds())}s"
def interval(a, b): return {"startTime": iso(a), "startUtcOffset": off(a), "endTime": iso(b), "endUtcOffset": off(b)}
def sample(t): return {"physicalTime": iso(t), "utcOffset": off(t)}
def ymd(d): return {"year": d.year, "month": d.month, "day": d.day}
def local(d, h, m=0): return datetime(d.year, d.month, d.day, tzinfo=TZ) + timedelta(hours=h, minutes=m)

# Slow trends with a bit of noise: fitness improves, a rough week mid-September.
def trend(i, base, amp, noise, period=23):
    return base + amp * math.sin(i / period * 2 * math.pi) + random.gauss(0, noise)

start_day = END - timedelta(days=DAYS - 1)
prev_wake = None
for i in range(DAYS):
    d = start_day + timedelta(days=i)
    mkey, dkey = d.strftime("%Y-%m"), d.isoformat()
    weekend = d.weekday() >= 5
    rough = 40 <= i <= 46
    rhr = trend(i, 57 - i * 0.03 + (3 if rough else 0), 1.2, 0.8)
    hrv = trend(i, 44 + i * 0.06 - (8 if rough else 0), 4, 3)
    party = d == PARTY
    night_hr = rhr + (9 if party else -2)
    if party:
        hrv, rhr = 24.0, rhr + 5

    # The night that ends this morning.
    bed = local(d - timedelta(days=1), 22, random.randint(30, 80) + (40 if weekend else 0))
    wake = local(d, 6, random.randint(20, 70) + (60 if weekend else 0))
    if party:
        bed, wake = local(d, 2, 40), local(d, 7, 50)
    if NOW < wake:
        continue
    t, stages, mins = bed, [], {"LIGHT": 0, "DEEP": 0, "REM": 0, "AWAKE": 0}
    cycle = 0
    while t < wake:
        for kind, length in (("LIGHT", random.randint(20, 35)), ("DEEP", random.randint(11, 14) if party else max(5, random.randint(35, 55) - cycle * 12)),
                             ("LIGHT", random.randint(10, 25)), ("REM", random.randint(10, 18) + cycle * 6),
                             ("AWAKE", random.choice([0, 0, 2, 4]))):
            if length == 0 or t >= wake:
                continue
            e = min(wake, t + timedelta(minutes=length))
            stages.append({**interval(t, e), "type": kind})
            mins[kind] += int((e - t).total_seconds() // 60)
            t = e
        cycle += 1
    asleep = mins["LIGHT"] + mins["DEEP"] + mins["REM"]
    put("sleep", mkey, {"sleep": {"interval": interval(bed, wake), "metadata": {"mainSleep": True}, "stages": stages,
        "summary": {"minutesAsleep": str(asleep), "minutesAwake": str(mins["AWAKE"]),
                    "stagesSummary": [{"type": k, "minutes": str(v)} for k, v in mins.items()]}}})

    # Night vitals.
    t = bed
    while t < wake:
        put("heart-rate", (t.date().isoformat()), {"heartRate": {"beatsPerMinute": str(round(night_hr + random.gauss(0, 2))), "sampleTime": sample(t)}})
        if (t - bed).seconds % 300 == 0:
            put("heart-rate-variability", t.date().isoformat(), {"heartRateVariability": {"rootMeanSquareOfSuccessiveDifferencesMilliseconds": round(max(15, hrv + random.gauss(0, 7)), 1), "sampleTime": sample(t)}})
        put("oxygen-saturation", t.date().isoformat(), {"oxygenSaturation": {"percentage": round(min(99.5, random.gauss(96.3, 0.9)), 1), "sampleTime": sample(t)}})
        t += timedelta(minutes=1)

    put("daily-resting-heart-rate", mkey, {"dailyRestingHeartRate": {"date": ymd(d), "beatsPerMinute": str(round(rhr))}})
    put("daily-heart-rate-variability", mkey, {"dailyHeartRateVariability": {"date": ymd(d), "averageHeartRateVariabilityMilliseconds": round(hrv, 1),
        "deepSleepRootMeanSquareOfSuccessiveDifferencesMilliseconds": round(hrv * 1.15, 1), "nonRemHeartRateBeatsPerMinute": round(night_hr, 1)}})
    put("daily-oxygen-saturation", mkey, {"dailyOxygenSaturation": {"date": ymd(d), "averagePercentage": round(random.gauss(96.2, 0.4), 1),
        "lowerBoundPercentage": round(random.gauss(93.5, 0.6), 1), "upperBoundPercentage": round(random.gauss(98.4, 0.3), 1)}})
    put("daily-respiratory-rate", mkey, {"dailyRespiratoryRate": {"date": ymd(d), "breathsPerMinute": round(trend(i, 14.2, 0.4, 0.3) + (0.8 if rough else 0), 1)}})
    put("daily-sleep-temperature-derivations", mkey, {"dailySleepTemperatureDerivations": {"date": ymd(d), "nightlyTemperatureCelsius": round(34.1 + random.gauss(0, 0.15) + (0.6 if rough else 0), 2)}})

    # The day: commute walks, an occasional workout.
    workout = None
    if not rough and (d.weekday() in (1, 3) or (weekend and random.random() < 0.7)):
        kind = random.choice([("RUNNING", "Run", 38, 150), ("BIKING", "Bike", 70, 135), ("RUNNING", "Run", 45, 152)]) if d.weekday() != 3 else ("WEIGHTLIFTING", "Weights", 50, 112)
        ws = local(d, 17 if not weekend else 9, random.randint(0, 50))
        workout = (ws, ws + timedelta(minutes=kind[2] + random.randint(-5, 10)), kind)
        we = workout[1]
        mins_active = int((we - ws).total_seconds() // 60)
        put("exercise", mkey, {"exercise": {"interval": interval(ws, we), "exerciseType": kind[0], "displayName": kind[1],
            "activeDuration": f"{mins_active * 60}s", "metricsSummary": {"caloriesKcal": round(mins_active * (11 if kind[0] != "WEIGHTLIFTING" else 6)),
            "averageHeartRateBeatsPerMinute": str(kind[3] + random.randint(-4, 4)), "activeZoneMinutes": str(round(mins_active * (1.6 if kind[0] != "WEIGHTLIFTING" else 0.6)))}}})
    walks = [(local(d, 7 + (2 if weekend else 0), 40), 18), (local(d, 12, 15), 22), (local(d, 17 if workout is None else 13, 45), 25)]
    end_day = min(local(d, 22, 30), NOW)
    t = wake
    while t < end_day:
        bpm = 68 + 6 * math.sin((t.hour - 9) / 24 * 2 * math.pi) + random.gauss(0, 3)
        steps = random.choice([0, 0, 0, 3, 8, 14]) if not weekend else random.choice([0, 0, 5, 12])
        kcal = 1.2 + random.random()
        azm = 0
        for ws, length in walks:
            if ws <= t < ws + timedelta(minutes=length):
                steps, bpm, kcal = random.randint(95, 118), bpm + 22, 4.5 + random.random()
                azm = 1 if random.random() < 0.4 else 0
        if workout and workout[0] <= t < workout[1]:
            target = workout[2][3]
            bpm = target + random.gauss(0, 6)
            steps = random.randint(150, 172) if workout[2][0] == "RUNNING" else 0
            kcal = 9 + 3 * random.random()
            azm = 2 if bpm > 140 else 1
        put("heart-rate", t.date().isoformat(), {"heartRate": {"beatsPerMinute": str(round(bpm)), "sampleTime": sample(t)}})
        e = t + timedelta(minutes=1)
        if steps:
            put("steps", t.date().isoformat(), {"steps": {"count": str(steps), "interval": interval(t, e)}})
        put("active-energy-burned", t.date().isoformat(), {"activeEnergyBurned": {"kcal": round(kcal, 2), "interval": interval(t, e)}})
        if azm:
            put("active-zone-minutes", t.date().isoformat(), {"activeZoneMinutes": {"activeZoneMinutes": str(azm), "heartRateZone": "CARDIO" if azm == 2 else "FAT_BURN", "interval": interval(t, e)}})
        t = e

n = 0
for (dtype, key), points in files.items():
    path = os.path.join(raw, dtype, f"{key}.jsonl")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        for p in points:
            f.write(json.dumps(p) + "\n")
            n += 1
print(f"{n} points in {len(files)} files")

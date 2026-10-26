// The public demo (/demo): the dashboard on made-up data, generated in the
// browser. Deterministic per date, so a day looks the same on every visit.
// Nothing here is anyone's data and nothing is sent anywhere.
import type { Daily, DayDetail, Exercise, Me, Night, Overview, Settings, Stage } from './api';

/** The app runs as the demo under /demo. */
export const DEMO = location.pathname === '/demo' || location.pathname.startsWith('/demo/');

const TZ = 'Europe/Prague';
const MIN = 60_000;
const DAY = 24 * 60 * MIN;
const DAYS = 90;

/** A small seeded random generator (mulberry32) and a roughly normal one on top. */
function random(seed: number) {
  let s = seed | 0;
  const r = () => {
    s = (s + 0x6d2b79f5) | 0;
    let t = Math.imul(s ^ (s >>> 15), 1 | s);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  const g = () => (r() + r() + r() + r() + r() + r() - 3) * 1.4;
  return { r, g };
}

const wall = new Intl.DateTimeFormat('sv-SE', { timeZone: TZ, year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit' });
/** Minutes the zone is ahead of UTC at `t`. */
const offsetMin = (t: number) => (Date.parse(wall.format(t).replace(' ', 'T') + 'Z') - Math.floor(t / 1000) * 1000) / MIN;
/** An instant as the API writes it: local wall time with its offset. */
function iso(t: number) {
  const o = offsetMin(t);
  const a = Math.abs(o);
  return `${wall.format(t).replace(' ', 'T')}${o >= 0 ? '+' : '-'}${String(Math.floor(a / 60)).padStart(2, '0')}:${String(a % 60).padStart(2, '0')}`;
}
/** The instant of a local wall time: `minutes` after midnight of `date` (negative: the evening before). */
function local(date: string, minutes: number) {
  const guess = Date.UTC(+date.slice(0, 4), +date.slice(5, 7) - 1, +date.slice(8, 10)) + minutes * MIN;
  const first = guess - offsetMin(guess) * MIN;
  return guess - offsetMin(first) * MIN;
}
const addDays = (d: string, n: number) => new Date(Date.parse(`${d}T12:00:00Z`) + n * DAY).toISOString().slice(0, 10);
const today = () => new Intl.DateTimeFormat('sv-SE', { timeZone: TZ }).format(Date.now());
const seedOf = (date: string) => Number(date.replaceAll('-', ''));

type Plan = { kind: string; name: string; start: number; minutes: number; hr: number };

/** What the made-up person did on `date`: runs every other day or so, a weekend ride, walks. */
function plans(date: string, r: () => number): Plan[] {
  const dow = new Date(`${date}T12:00:00Z`).getUTCDay();
  const out: Plan[] = [];
  const n = seedOf(date);
  if (n % 2 === 0 && dow !== 0) out.push({ kind: 'RUNNING', name: 'Running', start: 7 * 60 + 40 + Math.round(40 * r()), minutes: 32 + Math.round(20 * r()), hr: 150 + Math.round(8 * r()) });
  if (dow === 6) out.push({ kind: 'BIKING', name: 'Cycling', start: 10 * 60 + Math.round(60 * r()), minutes: 60 + Math.round(50 * r()), hr: 128 + Math.round(10 * r()) });
  if (dow === 0) out.push({ kind: n % 3 ? 'HIKING' : 'SWIMMING_POOL', name: '', start: 10 * 60 + Math.round(60 * r()), minutes: 45 + Math.round(60 * r()), hr: 112 + Math.round(10 * r()) });
  if (r() < 0.6) out.push({ kind: 'WALKING', name: 'Walking', start: 12 * 60 + 15 + Math.round(30 * r()), minutes: 18 + Math.round(15 * r()), hr: 98 });
  if (dow === 2) out.push({ kind: 'STRENGTH_TRAINING', name: '', start: 18 * 60 + Math.round(30 * r()), minutes: 45 + Math.round(15 * r()), hr: 112 });
  if (dow === 4) out.push({ kind: 'YOGA', name: '', start: 19 * 60 + Math.round(30 * r()), minutes: 40 + Math.round(20 * r()), hr: 84 });
  return out.sort((a, b) => a.start - b.start);
}

const details = new Map<string, DayDetail>();

/** One day in detail, as /api/day/{date} returns it. */
export function demoDay(date: string): DayDetail {
  const hit = details.get(date);
  if (hit) return hit;
  const { r, g } = random(seedOf(date));
  const dow = new Date(`${date}T12:00:00Z`).getUTCDay();
  const weekend = dow === 0 || dow === 6;
  // Slow ups and downs over weeks, so the trends have something to show.
  const n = Date.parse(`${date}T12:00:00Z`) / DAY;
  const drift = Math.sin(n / 13) + 0.5 * Math.sin(n / 5.3 + 1);
  // A rough night now and then, like the late one on the home page.
  const rough = r() < 0.08;
  const midnight = local(date, 0);
  const end = local(addDays(date, 1), 0);
  const sStart = local(date, (rough ? 80 : -55) + Math.round(35 * g()) + (weekend ? 30 : 0));
  const sEnd = local(date, (rough ? 6 * 60 + 30 : 7 * 60 + 5) + Math.round(25 * g()) + (weekend ? 50 : 0));

  type Block = { start: number; end: number; stage: Stage };
  const stages: Block[] = [];
  let t = sStart;
  const push = (stage: Stage, m: number) => {
    const e = Math.min(t + Math.max(1, Math.round(m)) * MIN, sEnd);
    if (e > t) stages.push({ start: t, end: e, stage });
    t = e;
  };
  push('LIGHT', 12 + 6 * r());
  for (let c = 0; t < sEnd; c++) {
    push('LIGHT', 12 + 8 * r());
    push('DEEP', Math.max(4, (rough ? 22 : 40) - c * 8 + 5 * g()));
    push('LIGHT', 14 + 8 * r());
    push('REM', 8 + c * 6 + 4 * r());
    if (r() < 0.45) push('AWAKE', 2 + 4 * r());
  }
  const restless = stages.filter((s) => s.stage === 'LIGHT' && s.end - s.start > 15 * MIN && r() < 0.35).map((s) => ({ start: s.start + 5 * MIN, end: s.start + 7 * MIN }));
  const stageAt = (x: number) => stages.find((s) => s.start <= x && x < s.end)?.stage;

  const ex = plans(date, r).map((p) => ({ ...p, s: local(date, p.start), e: local(date, p.start + p.minutes) }));
  const start = Math.min(sStart, midnight);
  const heartRate: DayDetail['heartRate'] = [];
  for (let x = start; x < end; x += MIN) {
    const st = stageAt(x);
    const e = ex.find((q) => q.s <= x && x < q.e);
    const after = ex.filter((q) => q.e <= x).at(-1);
    const recovery = after ? 25 * Math.exp(-(x - after.e) / (7 * MIN)) : 0;
    const v = st
      ? (rough ? 62 : 52 + 2.2 * drift) + (st === 'DEEP' ? -3 : st === 'REM' ? 4 : st === 'AWAKE' ? 9 : 0) + 1.5 * g()
      : e
        ? 72 + (e.hr - 72) * Math.min(1, (x - e.s) / (5 * MIN)) + (e.kind === 'STRENGTH_TRAINING' ? 12 * Math.sin((x - e.s) / (2.3 * MIN)) : 0) + 3.5 * g()
        : x < sStart
          ? 64 + 2 * g()
          : 67 + recovery + 3 * g();
    const a = Math.round(v);
    heartRate.push([iso(x), a - 2 - Math.round(Math.abs(2 * g())), a, a + 2 + Math.round(Math.abs(2 * g()))]);
  }
  const steps: DayDetail['steps'] = [];
  for (let x = midnight; x < end; x += 15 * MIN) {
    const e = ex.find((q) => q.s < x + 15 * MIN && x < q.e);
    const v = x < sEnd || x > local(date, 23 * 60) ? 0 : e?.kind === 'RUNNING' ? 2300 + Math.round(150 * g()) : e?.kind === 'WALKING' ? 1350 + Math.round(120 * g()) : Math.max(0, Math.round(110 + 120 * g() + (r() < 0.1 ? 500 * r() : 0)));
    steps.push([iso(x), v]);
  }
  const spo2: DayDetail['spo2'] = [];
  const hrv: DayDetail['hrv'] = [];
  for (let x = sStart + 10 * MIN; x < sEnd - 5 * MIN; x += MIN) {
    spo2.push([iso(x), Math.round((96.3 + 0.4 * drift + 0.7 * g() - (r() < 0.03 ? 2.5 * r() : 0)) * 10) / 10]);
    if ((x - sStart) % (5 * MIN) === 0) hrv.push([iso(x), Math.round(((rough ? 26 : 44 - 4 * drift) + (stageAt(x) === 'DEEP' ? 10 : stageAt(x) === 'REM' ? -5 : 0) + 6 * g()) * 10) / 10]);
  }

  const sm: Record<Stage, number> = { DEEP: 0, LIGHT: 0, REM: 0, AWAKE: 0 };
  for (const s of stages) sm[s.stage] += (s.end - s.start) / MIN;
  const asleep = sm.DEEP + sm.LIGHT + sm.REM;
  const hrs = heartRate.map((h) => h[2]);
  const nightHr = heartRate.filter((h) => stageAt(Date.parse(h[0]))).map((h) => h[2]);
  const totalSteps = steps.reduce((a, s) => a + s[1], 0);
  const azm = ex.reduce((a, q) => a + (q.hr >= 110 ? q.minutes : 0), 0) + Math.round(8 * r());
  const mean = (xs: number[]) => xs.reduce((a, b) => a + b, 0) / (xs.length || 1);

  const exercises: Exercise[] = ex.map((q) => ({
    start: iso(q.s), end: iso(q.e), kind: q.kind, name: q.name, active_minutes: q.minutes, avg_hr: q.hr,
    kcal: Math.round(q.minutes * (q.hr - 60) * 0.11), azm: q.hr >= 110 ? q.minutes : 0,
  }));
  const metrics: Daily = {
    date,
    resting_hr: Math.round(mean(nightHr) - 1 + g()),
    hrv_ms: Math.round(mean(hrv.map((h) => h[1])) * 10) / 10,
    hrv_deep_rmssd_ms: null,
    nonrem_hr: Math.round(mean(nightHr)),
    spo2_avg: Math.round(mean(spo2.map((s) => s[1])) * 10) / 10,
    spo2_low: Math.min(...spo2.map((s) => s[1])),
    spo2_high: Math.max(...spo2.map((s) => s[1])),
    respiratory_rate: Math.round(((rough ? 14.6 : 13.6 + 0.3 * drift) + 0.3 * g()) * 10) / 10,
    nightly_temp_c: Math.round((33.7 + 0.2 * g() + (rough ? 0.4 : 0)) * 100) / 100,
    steps: totalSteps,
    active_kcal: Math.round(totalSteps * 0.045 + azm * 7 + 300),
    azm,
    hr_min: Math.min(...hrs),
    hr_avg: Math.round(mean(hrs) * 10) / 10,
    hr_max: Math.max(...hrs),
    sleep_minutes: asleep,
    sleep_awake_minutes: sm.AWAKE,
    sleep_deep_minutes: sm.DEEP,
    sleep_light_minutes: sm.LIGHT,
    sleep_rem_minutes: sm.REM,
    sleep_start: iso(sStart),
    sleep_end: iso(sEnd),
    exercise_minutes: ex.reduce((a, q) => a + q.minutes, 0),
  };
  const sleep: Night = {
    date, start: iso(sStart), end: iso(sEnd), minutes_asleep: asleep, minutes_awake: sm.AWAKE,
    stages: stages.map((s) => ({ start: iso(s.start), end: iso(s.end), stage: s.stage })),
    stage_minutes: sm,
    restless: restless.map((s) => ({ start: iso(s.start), end: iso(s.end) })),
    restless_minutes: restless.length * 2,
  };
  const day: DayDetail = { date, timeZone: TZ, range: [iso(start), iso(end)], midnight: iso(midnight), metrics, heartRate, steps, spo2, hrv, sleep, exercises };
  details.set(date, day);
  return day;
}

let settings: Settings = { timeZone: TZ, units: 'metric', weekStart: 'monday', clock: '24h' };
const user = (): Me => ({ id: 'demo', connected: true, settings, settingsSaved: true });

/** /api/overview: 90 days ending yesterday. */
export function demoOverview(): Overview {
  const last = addDays(today(), -1);
  const days = Array.from({ length: DAYS }, (_, i) => demoDay(addDays(last, i - DAYS + 1)).metrics!);
  return { days, lastSync: { run_at: new Date(Date.now() - 12 * MIN).toISOString().slice(0, 19), ok: true, auth_error: false }, user: user() };
}

/** Settings in the demo apply in this browser tab only. */
export function demoSaveSettings(s: Settings): Me {
  settings = s;
  return user();
}

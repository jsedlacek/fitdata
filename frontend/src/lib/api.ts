// Types and fetchers for the Rust API (src/web.rs).

export type Daily = {
  date: string;
  resting_hr: number | null;
  hrv_ms: number | null;
  hrv_deep_rmssd_ms: number | null;
  nonrem_hr: number | null;
  spo2_avg: number | null;
  spo2_low: number | null;
  spo2_high: number | null;
  respiratory_rate: number | null;
  nightly_temp_c: number | null;
  steps: number | null;
  active_kcal: number | null;
  azm: number | null;
  hr_min: number | null;
  hr_avg: number | null;
  hr_max: number | null;
  sleep_minutes: number | null;
  sleep_awake_minutes: number | null;
  sleep_deep_minutes: number | null;
  sleep_light_minutes: number | null;
  sleep_rem_minutes: number | null;
  sleep_start: string | null;
  sleep_end: string | null;
  exercise_minutes: number | null;
};

export type SyncRun = { run_at: string; ok: boolean; auth_error: boolean };

export type Overview = { days: Daily[]; lastSync: SyncRun | null };

export type Stage = 'DEEP' | 'LIGHT' | 'REM' | 'AWAKE';

export type Night = {
  date: string;
  start: string;
  end: string;
  minutes_asleep: number | null;
  minutes_awake: number | null;
  stages: { start: string; end: string; stage: Stage }[];
  stage_minutes: Partial<Record<Stage, number | null>>;
};

export type Exercise = {
  start: string;
  end: string;
  kind: string;
  name: string;
  active_minutes: number | null;
  avg_hr: number | null;
  kcal: number | null;
  azm: number | null;
};

export type DayDetail = {
  date: string;
  range: [string, string];
  metrics: Daily | null;
  /** [minute, min, mean, max] */
  heartRate: [string, number, number, number][];
  /** [15-minute bin, steps] */
  steps: [string, number][];
  spo2: [string, number][];
  hrv: [string, number][];
  sleep: Night | null;
  exercises: Exercise[];
};

export class UnauthorizedError extends Error {}

async function get<T>(url: string): Promise<T> {
  const r = await fetch(url, { credentials: 'same-origin' });
  if (r.status === 401) {
    // Session expired: sign in again and come back.
    location.href = '/auth/login';
    throw new UnauthorizedError('signed out');
  }
  if (!r.ok) throw new Error(`${url}: HTTP ${r.status}`);
  return r.json() as Promise<T>;
}

export const overviewQuery = {
  queryKey: ['overview'],
  queryFn: () => get<Overview>('/api/overview'),
  staleTime: 5 * 60_000,
};

export const dayQuery = (date: string) => ({
  queryKey: ['day', date],
  queryFn: () => get<DayDetail>(`/api/day/${date}`),
  staleTime: 5 * 60_000,
});

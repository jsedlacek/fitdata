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

/** How far a first sync has got (the job's `status::Progress`). */
export type SyncProgress =
  | { step: 'recent' }
  | { step: 'history'; done: number; total: number }
  | { step: 'tables' };
export type SyncState = { running: boolean; progress?: SyncProgress | null };

export type Units = 'metric' | 'imperial';
export type WeekStart = 'monday' | 'sunday' | 'saturday';
export type Clock = '24h' | '12h';

/** The Settings page's values. */
export type Settings = { timeZone: string; units: Units; weekStart: WeekStart; clock: Clock };

/**
 * The signed-in user; `connected` once Fitbit data may be read.
 * `settingsSaved` is false until the settings were chosen (or detected from the browser) once.
 */
export type Me = { id: string; connected: boolean; settings: Settings; settingsSaved: boolean };

export type Overview = { days: Daily[]; lastSync: SyncRun | null; user: Me };

export type Stage = 'DEEP' | 'LIGHT' | 'REM' | 'AWAKE';

export type Night = {
  date: string;
  start: string;
  end: string;
  minutes_asleep: number | null;
  minutes_awake: number | null;
  stages: { start: string; end: string; stage: Stage }[];
  stage_minutes: Partial<Record<Stage, number | null>>;
  /** Short awakenings within light sleep (restless sleep in Google Health). */
  restless?: { start: string; end: string }[];
  restless_minutes?: number | null;
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

/**
 * Times are instants with the UTC offset of the user's time zone,
 * e.g. `2026-10-25T02:30:00+01:00`: the first 16 characters are the local
 * wall-clock time, `Date.parse` gives the instant.
 */
export type DayDetail = {
  date: string;
  /** IANA time zone the times' offsets follow. */
  timeZone: string;
  range: [string, string];
  /** The midnight `date` starts at. */
  midnight: string;
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

async function request<T>(url: string, method = 'GET', body?: unknown): Promise<T> {
  const r = await fetch(url, {
    method,
    credentials: 'same-origin',
    ...(body !== undefined && { headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }),
  });
  if (r.status === 401) {
    // Session expired: sign in again and come back.
    location.href = '/auth/login';
    throw new UnauthorizedError('signed out');
  }
  if (!r.ok) throw new Error(`${url}: HTTP ${r.status}`);
  return r.json() as Promise<T>;
}

const get = <T,>(url: string) => request<T>(url);

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

/** Whether a sync job runs: polled often while it does, otherwise now and then to notice the scheduled one. */
export const syncQuery = {
  queryKey: ['sync'],
  queryFn: () => get<SyncState>('/api/sync'),
  refetchInterval: (q: { state: { data?: SyncState } }) => (q.state.data?.running ? 5_000 : 5 * 60_000),
};

/** Starts a sync job (or joins the one already running). */
export const startSync = () => request<SyncState>('/api/sync', 'POST');

/** Saves the settings; a new time zone also starts a sync that recalculates the data. */
export const saveSettings = (s: Settings) => request<Me>('/api/settings', 'PUT', s);

/** Everything stored about the user, as a ZIP (GDPR export). */
export const EXPORT_URL = '/api/export';

/** Deletes the account and all its data, then signs out. */
export async function deleteAccount() {
  await request<unknown>('/api/account', 'DELETE');
  location.href = '/auth/logout';
}

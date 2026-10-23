import type { Daily } from './api';

export type MetricKey = Exclude<keyof Daily, 'date' | 'sleep_start' | 'sleep_end'>;

export function mean(xs: number[]): number | null {
  return xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : null;
}

/** Values of `key` for the `n` days before `date` (oldest first, missing days skipped). */
export function before(days: Daily[], date: string, key: MetricKey, n: number): number[] {
  return days
    .filter((d) => d.date < date)
    .slice(-n)
    .map((d) => d[key])
    .filter((v): v is number => v != null);
}

/** Trailing mean over up to `window` previous values that exist (incl. the current one). */
export function rolling(values: (number | null)[], window: number): (number | null)[] {
  return values.map((_, i) => {
    const xs = values.slice(Math.max(0, i - window + 1), i + 1).filter((v): v is number => v != null);
    return xs.length >= Math.min(3, window) ? mean(xs) : null;
  });
}

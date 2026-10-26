import { prefs } from './prefs';

export const MIN = 60_000;
export const HOUR = 60 * MIN;
export const DAY = 24 * HOUR;

/**
 * `HH:mm` (or `h:mm am`) of an instant on the wall clock of `tz`.
 * The hour repeated when clocks go back is on the axis twice, so a time in
 * it gets its zone: `02:30 CEST`, then `02:30 CET`.
 */
export function wallClock(tz: string) {
  const hm = new Intl.DateTimeFormat('en-GB', {
    ...(prefs().clock === '12h' ? { hour: 'numeric', hourCycle: 'h12' } : { hour: '2-digit', hourCycle: 'h23' }),
    minute: '2-digit',
    timeZone: tz,
  });
  const zone = new Intl.DateTimeFormat('en-GB', { timeZone: tz, timeZoneName: 'short' });
  return (t: number | string) => {
    const v = typeof t === 'string' ? Date.parse(t) : t;
    const s = hm.format(v);
    const repeated = hm.format(v - HOUR) === s || hm.format(v + HOUR) === s;
    return repeated ? `${s} ${zone.formatToParts(v).find((p) => p.type === 'timeZoneName')?.value ?? ''}`.trim() : s;
  };
}

/** Minutes after local midnight of an instant in `tz`. */
export function localMinutes(tz: string) {
  const f = new Intl.DateTimeFormat('en-GB', { hour: '2-digit', minute: '2-digit', hourCycle: 'h23', timeZone: tz });
  return (t: number) => {
    const p = f.formatToParts(t);
    return Number(p.find((x) => x.type === 'hour')!.value) * 60 + Number(p.find((x) => x.type === 'minute')!.value);
  };
}

/** A "YYYY-MM-DD" date as an x value on a date axis: its UTC midnight. */
export const dayValue = (d: string) => Date.UTC(Number(d.slice(0, 4)), Number(d.slice(5, 7)) - 1, Number(d.slice(8, 10)));

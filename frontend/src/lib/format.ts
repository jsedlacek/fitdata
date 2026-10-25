// Number, date and duration formatting (British English: 24-hour clock, day before month).

const nf = new Map<number, Intl.NumberFormat>();
export function num(v: number | null | undefined, digits = 0): string {
  if (v == null || Number.isNaN(v)) return '–';
  let f = nf.get(digits);
  if (!f) {
    f = new Intl.NumberFormat('en-GB', { minimumFractionDigits: digits, maximumFractionDigits: digits });
    nf.set(digits, f);
  }
  return f.format(v);
}

/** 435 -> "7:15" */
export function hm(minutes: number | null | undefined): string {
  if (minutes == null) return '–';
  const m = Math.round(minutes);
  return `${Math.floor(m / 60)}:${String(m % 60).padStart(2, '0')}`;
}

/** 435 -> "7 h 15 min", 25 -> "25 min" */
export function duration(minutes: number | null | undefined): string {
  if (minutes == null) return '–';
  const m = Math.round(minutes);
  if (m < 60) return `${m} min`;
  return `${Math.floor(m / 60)} h ${String(m % 60).padStart(2, '0')} min`;
}

/** "2026-10-23T06:58:00+02:00" -> "06:58", the local wall-clock time */
export const clock = (s: string) => s.slice(11, 16);

/** Local date from "YYYY-MM-DD" (noon avoids DST edge cases). */
export const parseDate = (d: string) => new Date(`${d}T12:00:00`);

export function isoDate(d: Date): string {
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export function addDays(d: string, n: number): string {
  const x = parseDate(d);
  x.setDate(x.getDate() + n);
  return isoDate(x);
}

export const weekdayShort = (d: string) =>
  parseDate(d).toLocaleDateString('en-GB', { weekday: 'short', day: 'numeric', month: 'short' });

export const dateLong = (d: string) =>
  parseDate(d).toLocaleDateString('en-GB', { weekday: 'long', day: 'numeric', month: 'long' });

export function relativeDay(d: string, today = isoDate(new Date())): string | null {
  if (d === today) return 'Today';
  if (d === addDays(today, -1)) return 'Yesterday';
  return null;
}

/** "3 h ago", "2 days ago" for a local "YYYY-MM-DDTHH:MM:SS". */
export function ago(at: string): string {
  const mins = Math.round((Date.now() - Date.parse(at)) / 60_000);
  if (mins < 1) return 'just now';
  if (mins < 60) return `${mins} min ago`;
  const h = Math.round(mins / 60);
  if (h < 24) return `${h} h ago`;
  const d = Math.round(h / 24);
  return `${d} ${plural(d, 'day', 'days')} ago`;
}

export function plural(n: number, one: string, many: string) {
  return n === 1 ? one : many;
}

// The user's display settings (Settings page), read by the formatting helpers.
// AppShell sets them from the overview before rendering a page.

import type { Clock, Settings, Units, WeekStart } from './api';

let current: Settings = { timeZone: 'Europe/Prague', units: 'metric', weekStart: 'monday', clock: '24h' };

export const prefs = () => current;

export function setPrefs(s: Settings) {
  current = s;
}

/** Countries that measure in miles, pounds and °F. */
const IMPERIAL = ['US', 'LR', 'MM'];
/** Where weeks start on Sunday or Saturday, for browsers without `Intl.Locale#getWeekInfo`. */
const SUNDAY = ['US', 'CA', 'MX', 'BR', 'JP', 'KR', 'TW', 'HK', 'IL', 'PH', 'IN', 'ZA', 'SA'];
const SATURDAY = ['AE', 'EG', 'IR', 'IQ', 'JO', 'KW', 'LY', 'OM', 'QA', 'SY', 'AF', 'DZ', 'BH'];

type WeekInfo = { firstDay: number };
type LocaleWithWeek = Intl.Locale & { getWeekInfo?: () => WeekInfo; weekInfo?: WeekInfo };

/** Sensible settings for a new user, from the browser's time zone and language. */
export function detectSettings(): Settings {
  const lang = navigator.language || 'en-GB';
  let region = '';
  let firstDay: number | undefined;
  try {
    const locale = new Intl.Locale(lang).maximize() as LocaleWithWeek;
    region = locale.region ?? '';
    firstDay = (locale.getWeekInfo?.() ?? locale.weekInfo)?.firstDay;
  } catch {
    // An odd language tag: the defaults below.
  }
  const units: Units = IMPERIAL.includes(region) ? 'imperial' : 'metric';
  const weekStart: WeekStart =
    firstDay === 7 || (firstDay == null && SUNDAY.includes(region))
      ? 'sunday'
      : firstDay === 6 || (firstDay == null && SATURDAY.includes(region))
        ? 'saturday'
        : 'monday';
  const cycle = new Intl.DateTimeFormat(lang, { hour: 'numeric' }).resolvedOptions().hourCycle;
  const clock: Clock = cycle === 'h12' || cycle === 'h11' ? '12h' : '24h';
  return { timeZone: browserTimeZone(), units, weekStart, clock };
}

export function browserTimeZone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone || 'Europe/Prague';
}

/** All IANA time zones the browser knows, for the picker. */
export function timeZones(): string[] {
  try {
    return Intl.supportedValuesOf('timeZone');
  } catch {
    return [browserTimeZone()];
  }
}

/** 0 = Sunday … 6 = Saturday, as `Date#getDay`. */
export const firstWeekday = () => ({ sunday: 0, monday: 1, saturday: 6 })[current.weekStart];

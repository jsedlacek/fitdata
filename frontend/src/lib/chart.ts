import { cssVar } from './theme';

/** Shared chart chrome read from the theme's CSS variables. */
export function chrome() {
  const c = {
    ink: cssVar('--ink'),
    ink2: cssVar('--ink-2'),
    muted: cssVar('--muted'),
    grid: cssVar('--grid'),
    axis: cssVar('--axis'),
    surface: cssVar('--surface'),
    border: cssVar('--border'),
    shade: cssVar('--shade'),
    band: cssVar('--band'),
    s1: cssVar('--s1'),
    s2: cssVar('--s2'),
    s3: cssVar('--s3'),
    s4: cssVar('--s4'),
    s5: cssVar('--s5'),
    s7: cssVar('--s7'),
  };
  const axisLabel = { color: c.muted, fontSize: 11, fontFamily: 'system-ui, sans-serif' };
  return {
    c,
    textStyle: { fontFamily: 'system-ui, -apple-system, "Segoe UI", sans-serif', color: c.ink2 },
    tooltip: {
      backgroundColor: c.surface,
      borderColor: c.border,
      borderWidth: 1,
      padding: [8, 10],
      textStyle: { color: c.ink, fontSize: 12 },
      extraCssText: 'border-radius:10px;box-shadow:0 6px 20px rgba(0,0,0,.12);',
    },
    /** Time axis labelled with the wall clock of time zone `tz`. */
    timeAxis: (tz: string) => ({
      type: 'time' as const,
      axisLabel: { ...axisLabel, hideOverlap: true, formatter: wallClock(tz) },
      axisLine: { lineStyle: { color: c.axis } },
      axisTick: { show: false },
      splitLine: { show: false },
    }),
    valueAxis: {
      type: 'value' as const,
      axisLabel,
      splitNumber: 3,
      splitLine: { lineStyle: { color: c.grid } },
      axisLine: { show: false },
    },
    axisLabel,
  };
}

/**
 * `HH:mm` of a time from the API (an instant) on the wall clock of `tz`.
 * The hour repeated when clocks go back is on the axis twice, so a time in
 * it gets its zone: `02:30 SELČ`, then `02:30 SEČ`.
 */
export function wallClock(tz: string) {
  const hm = new Intl.DateTimeFormat('cs-CZ', { hour: '2-digit', minute: '2-digit', hourCycle: 'h23', timeZone: tz });
  const zone = new Intl.DateTimeFormat('cs-CZ', { timeZone: tz, timeZoneName: 'short' });
  return (t: number | string) => {
    const v = typeof t === 'string' ? Date.parse(t) : t;
    const s = hm.format(v);
    const repeated = hm.format(v - 3_600_000) === s || hm.format(v + 3_600_000) === s;
    return repeated ? `${s} ${zone.formatToParts(v).find((p) => p.type === 'timeZoneName')?.value ?? ''}`.trim() : s;
  };
}

/** Minutes between two times from the API. */
export const minutesBetween = (a: string, b: string) => (Date.parse(b) - Date.parse(a)) / 60_000;

/** HTML for a tooltip row: colored marker + label + value. */
export const row = (color: string, label: string, value: string) =>
  `<div style="display:flex;align-items:center;gap:8px;margin-top:2px"><span style="width:8px;height:8px;border-radius:2px;background:${color}"></span><span style="flex:1;opacity:.75">${label}</span><b style="font-variant-numeric:tabular-nums">${value}</b></div>`;

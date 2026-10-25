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
      axisLabel: { ...axisLabel, hideOverlap: true, formatter: hhmm(tz) },
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
 * `HH:mm` of an instant on the wall clock of `tz`. Times from the API are
 * instants with their offset, so the hour repeated when clocks go back is
 * on the axis twice (02:00, 02:00), not one hour drawn over the other.
 */
function hhmm(tz: string) {
  const f = new Intl.DateTimeFormat('cs-CZ', { hour: '2-digit', minute: '2-digit', hourCycle: 'h23', timeZone: tz });
  return (v: number) => f.format(v);
}

/** Minutes between two times from the API. */
export const minutesBetween = (a: string, b: string) => (Date.parse(b) - Date.parse(a)) / 60_000;

/** HTML for a tooltip row: colored marker + label + value. */
export const row = (color: string, label: string, value: string) =>
  `<div style="display:flex;align-items:center;gap:8px;margin-top:2px"><span style="width:8px;height:8px;border-radius:2px;background:${color}"></span><span style="flex:1;opacity:.75">${label}</span><b style="font-variant-numeric:tabular-nums">${value}</b></div>`;

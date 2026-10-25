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
    timeAxis: {
      type: 'time' as const,
      axisLabel: { ...axisLabel, hideOverlap: true, formatter: '{HH}:{mm}' },
      axisLine: { lineStyle: { color: c.axis } },
      axisTick: { show: false },
      splitLine: { show: false },
    },
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
 * The API's local wall-clock time as an ECharts instant. Charts plot and label
 * it in UTC (`useUTC` in EChart), so the axis follows the wall clock and a DST
 * change neither opens nor swallows an hour.
 */
export const wall = (s: string) => `${s}Z`;

/** Minutes between two wall-clock times. */
export const wallMinutes = (a: string, b: string) => (Date.parse(wall(b)) - Date.parse(wall(a))) / 60_000;

/** HTML for a tooltip row: colored marker + label + value. */
export const row = (color: string, label: string, value: string) =>
  `<div style="display:flex;align-items:center;gap:8px;margin-top:2px"><span style="width:8px;height:8px;border-radius:2px;background:${color}"></span><span style="flex:1;opacity:.75">${label}</span><b style="font-variant-numeric:tabular-nums">${value}</b></div>`;

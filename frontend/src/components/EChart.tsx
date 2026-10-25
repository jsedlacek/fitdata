import { useEffect, useRef } from 'react';
import * as echarts from 'echarts/core';
import { BarChart, CustomChart, LineChart, ScatterChart } from 'echarts/charts';
import {
  AxisPointerComponent,
  DataZoomComponent,
  GridComponent,
  MarkAreaComponent,
  MarkLineComponent,
  TooltipComponent,
} from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import { cn } from '@/lib/utils';

echarts.use([
  BarChart, CustomChart, LineChart, ScatterChart,
  AxisPointerComponent, DataZoomComponent, GridComponent, MarkAreaComponent, MarkLineComponent, TooltipComponent,
  CanvasRenderer,
]);

export type Option = echarts.EChartsCoreOption;

/** Thin ECharts wrapper: one instance per element, resized with its container. */
export function EChart({ option, className, label }: { option: Option; className?: string; label: string }) {
  const el = useRef<HTMLDivElement>(null);
  const chart = useRef<echarts.ECharts | null>(null);

  useEffect(() => {
    const c = echarts.init(el.current!, null, { renderer: 'canvas' });
    chart.current = c;
    const ro = new ResizeObserver(() => c.resize());
    ro.observe(el.current!);
    return () => {
      ro.disconnect();
      c.dispose();
      chart.current = null;
    };
  }, []);

  useEffect(() => {
    // Time axes carry wall-clock times as UTC instants, see `wall`.
    chart.current?.setOption({ useUTC: true, ...option }, { notMerge: true });
  }, [option]);

  return <div ref={el} role="img" aria-label={label} className={cn('w-full', className)} />;
}

import type { ReactNode } from 'react';

/** A flat page section: title, optional note, a hairline, then the content. */
export function Section({ title, note, action, children, className }: { title: ReactNode; note?: ReactNode; action?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={className}>
      <div className="mb-3 flex flex-wrap items-end justify-between gap-x-4 gap-y-2 border-b pb-2">
        <div>
          <h2 className="text-[15px] font-semibold tracking-tight">{title}</h2>
          {note && <p className="mt-0.5 text-xs text-muted">{note}</p>}
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}

/** A table cell's bar: a thin filled bar on a faint track. */
export function InlineBar({ value, max, color = 'var(--s1)' }: { value: number; max: number; color?: string }) {
  return (
    <div className="h-1.5 w-full rounded-full bg-[color-mix(in_srgb,var(--ink)_6%,transparent)]">
      <div className="h-full rounded-full" style={{ width: `${Math.max(1.5, (value / (max || 1)) * 100)}%`, background: color }} />
    </div>
  );
}

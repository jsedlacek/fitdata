// Building blocks of the docs' pages. Pages are plain JSX made of these and of HTML
// elements, which the `.docs` styles (index.css) set as long-form text.
import type { ReactNode } from 'react';
import { Info, Lightbulb, Stethoscope, TriangleAlert } from 'lucide-react';
import { href, onLinkClick } from './nav';
import { cn } from '@/lib/utils';

/** A link to another docs page (`"hrv"`, `"hrv#reading"`, `"#section"`), or out of the docs. */
export function A({ to, children }: { to: string; children: ReactNode }) {
  if (/^(https?:|mailto:|\/)/.test(to)) return <a href={to}>{children}</a>;
  return (
    <a href={href(to)} onClick={(e) => onLinkClick(e, to)}>
      {children}
    </a>
  );
}

function Heading({ as: H, id, children }: { as: 'h2' | 'h3'; id: string; children: ReactNode }) {
  return (
    <H id={id} className="group">
      {children}
      <a href={`#${id}`} onClick={(e) => onLinkClick(e, `#${id}`)} className="anchor" aria-label="Link to this section">
        #
      </a>
    </H>
  );
}

export const H2 = (p: { id: string; children: ReactNode }) => <Heading as="h2" {...p} />;
export const H3 = (p: { id: string; children: ReactNode }) => <Heading as="h3" {...p} />;

const CALLOUT = {
  note: { Icon: Info, color: 'var(--s1)', title: 'Note' },
  tip: { Icon: Lightbulb, color: 'var(--s3)', title: 'Tip' },
  warn: { Icon: TriangleAlert, color: 'var(--warning)', title: 'Keep in mind' },
  medical: { Icon: Stethoscope, color: 'var(--critical)', title: 'Not medical advice' },
};

export function Callout({ kind = 'note', title, children }: { kind?: keyof typeof CALLOUT; title?: string; children: ReactNode }) {
  const c = CALLOUT[kind];
  return (
    <div className="callout" style={{ ['--c' as string]: c.color }}>
      <c.Icon className="mt-0.5 size-4 shrink-0" style={{ color: c.color }} aria-hidden />
      <div className="min-w-0">
        <p className="callout-title">{title ?? c.title}</p>
        {children}
      </div>
    </div>
  );
}

/** Key facts of a metric: a compact two-column list at the top of its page. */
export function Facts({ items }: { items: [string, ReactNode][] }) {
  return (
    <dl className="facts">
      {items.map(([k, v]) => (
        <div key={k}>
          <dt>{k}</dt>
          <dd>{v}</dd>
        </div>
      ))}
    </dl>
  );
}

/**
 * A scale from `min` to `max` with the usual band highlighted, e.g. resting heart
 * rate 60–100 bpm for adults. `marks` are labelled points on the scale.
 */
export function TypicalRange({
  min,
  max,
  band,
  unit,
  label,
  marks = [],
  color = 'var(--s3)',
}: {
  min: number;
  max: number;
  band: [number, number];
  unit: string;
  label: string;
  marks?: { at: number; label: string }[];
  color?: string;
}) {
  const pct = (v: number) => `${((v - min) / (max - min)) * 100}%`;
  return (
    <figure className="not-docs my-6 rounded-xl border bg-surface px-4 pb-4 pt-3">
      <figcaption className="mb-6 text-[13px] text-ink-2">{label}</figcaption>
      <div className="relative mx-2 h-2 rounded-full bg-[color-mix(in_srgb,var(--ink)_7%,transparent)]">
        <div className="absolute inset-y-0 rounded-full" style={{ left: pct(band[0]), width: `calc(${pct(band[1])} - ${pct(band[0])})`, background: color }} />
        {[band[0], band[1]].map((v) => (
          <span key={v} className="absolute -top-5 -translate-x-1/2 text-[11px] font-medium tabular-nums" style={{ left: pct(v) }}>
            {v}
          </span>
        ))}
        {marks.map((m) => (
          <span key={m.label} className="absolute top-0 h-2 w-px bg-ink-2" style={{ left: pct(m.at) }}>
            <span className="absolute top-3 -translate-x-1/2 whitespace-nowrap text-[11px] text-muted">{m.label}</span>
          </span>
        ))}
      </div>
      <div className={cn('mx-2 flex justify-between text-[11px] tabular-nums text-muted', marks.length ? 'mt-7' : 'mt-2')}>
        <span>
          {min} {unit}
        </span>
        <span>
          {max} {unit}
        </span>
      </div>
    </figure>
  );
}

/** A color chip as used in the charts, with its name. */
export function Swatch({ color, children }: { color: string; children: ReactNode }) {
  return (
    <span className="inline-flex items-center gap-1.5 whitespace-nowrap">
      <i className="inline-block size-2.5 rounded-[3px] align-[-1px]" style={{ background: color }} />
      {children}
    </span>
  );
}

export const Kbd = ({ children }: { children: ReactNode }) => <kbd>{children}</kbd>;

/** A screenshot in light and dark versions, matching the reader's theme. */
export function Shot({ light, dark, alt, caption }: { light: string; dark: string; alt: string; caption?: ReactNode }) {
  return (
    <figure className="not-docs my-6">
      <div className="overflow-hidden rounded-xl border bg-surface">
        <img src={light} alt={alt} className="block h-auto w-full dark-hide" loading="lazy" />
        <img src={dark} alt={alt} className="hidden h-auto w-full dark-show" loading="lazy" />
      </div>
      {caption && <figcaption className="mt-2 text-center text-xs text-muted">{caption}</figcaption>}
    </figure>
  );
}

/** Cards linking to pages, for overviews. */
export function Cards({ items }: { items: { to: string; title: string; text: string }[] }) {
  return (
    <div className="not-docs my-6 grid gap-3 sm:grid-cols-2">
      {items.map((i) => (
        <a key={i.to} href={href(i.to)} onClick={(e) => onLinkClick(e, i.to)} className="rounded-xl border bg-surface p-4 transition-colors hover:bg-shade">
          <span className="block text-[15px] font-semibold">{i.title}</span>
          <span className="mt-1 block text-sm text-ink-2">{i.text}</span>
        </a>
      ))}
    </div>
  );
}

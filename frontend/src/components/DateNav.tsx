import { useMemo, useState } from 'react';
import { CalendarDays, ChevronLeft, ChevronRight } from 'lucide-react';
import { addDays, dateLong, isoDate, parseDate, relativeDay } from '@/lib/format';
import { firstWeekday } from '@/lib/prefs';
import { cn } from '@/lib/utils';
import { Button } from './ui/button';
import { Popover, PopoverContent, PopoverTrigger } from './ui/popover';

/**
 * The page's title: the date (a button that opens a month calendar marking days
 * with data), "Today" or "Yesterday" above it, and previous / next day with data.
 */
export function DateNav({ date, available, onChange }: { date: string; available: string[]; onChange: (d: string) => void }) {
  const idx = available.indexOf(date);
  const prev = idx > 0 ? available[idx - 1] : undefined;
  const next = idx >= 0 && idx < available.length - 1 ? available[idx + 1] : undefined;
  const [open, setOpen] = useState(false);
  const rel = relativeDay(date);
  const step = 'size-9 rounded-full text-ink-2 hover:text-ink';

  return (
    <div className="flex items-end justify-between gap-3">
      <div className="min-w-0">
        <div className="h-4 text-xs font-medium text-muted">{rel}</div>
        <Popover open={open} onOpenChange={setOpen}>
          <h1 className="mt-0.5">
            <PopoverTrigger asChild>
              <button className="group inline-flex max-w-full cursor-pointer items-center gap-1.5 rounded-md text-[22px] font-semibold tracking-tight sm:text-2xl" aria-label={`${dateLong(date)}, choose another day`}>
                <span className="truncate first-letter:uppercase">{dateLong(date)}</span>
                <CalendarDays className="size-4 shrink-0 text-muted transition-colors group-hover:text-ink" />
              </button>
            </PopoverTrigger>
          </h1>
          <PopoverContent align="start" className="w-[288px]">
            <MiniCalendar
              selected={date}
              available={available}
              onSelect={(d) => {
                onChange(d);
                setOpen(false);
              }}
            />
          </PopoverContent>
        </Popover>
      </div>
      <div className="flex shrink-0 gap-1">
        <Button variant="outline" size="icon" className={step} disabled={!prev} onClick={() => prev && onChange(prev)} aria-label="Previous day">
          <ChevronLeft />
        </Button>
        <Button variant="outline" size="icon" className={step} disabled={!next} onClick={() => next && onChange(next)} aria-label="Next day">
          <ChevronRight />
        </Button>
      </div>
    </div>
  );
}

/** By `Date#getDay`: Sunday first. */
const WEEKDAYS = ['Su', 'Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa'];

function MiniCalendar({ selected, available, onSelect }: { selected: string; available: string[]; onSelect: (d: string) => void }) {
  const [month, setMonth] = useState(() => selected.slice(0, 7));
  const has = useMemo(() => new Set(available), [available]);
  const today = isoDate(new Date());

  const first = parseDate(`${month}-01`);
  const start = firstWeekday();
  const offset = (first.getDay() - start + 7) % 7;
  const weekdays = [...WEEKDAYS.slice(start), ...WEEKDAYS.slice(0, start)];
  const daysInMonth = new Date(first.getFullYear(), first.getMonth() + 1, 0).getDate();
  const cells = [...Array(offset).fill(null), ...Array.from({ length: daysInMonth }, (_, i) => `${month}-${String(i + 1).padStart(2, '0')}`)];
  const shift = (n: number) => {
    const d = parseDate(`${month}-01`);
    d.setMonth(d.getMonth() + n);
    setMonth(isoDate(d).slice(0, 7));
  };

  return (
    <div>
      <div className="mb-2 flex items-center justify-between">
        <Button variant="ghost" size="icon-sm" onClick={() => shift(-1)} aria-label="Previous month">
          <ChevronLeft />
        </Button>
        <div className="text-sm font-medium first-letter:uppercase">
          {first.toLocaleDateString('en-GB', { month: 'long', year: 'numeric' })}
        </div>
        <Button variant="ghost" size="icon-sm" onClick={() => shift(1)} aria-label="Next month">
          <ChevronRight />
        </Button>
      </div>
      <div className="grid grid-cols-7 gap-0.5 text-center text-[11px] text-muted">
        {weekdays.map((w) => (
          <div key={w} className="py-1">{w}</div>
        ))}
      </div>
      <div className="grid grid-cols-7 gap-0.5">
        {cells.map((d, i) =>
          d ? (
            <button
              key={d}
              disabled={!has.has(d)}
              onClick={() => onSelect(d)}
              className={cn(
                'relative h-9 rounded-md text-sm tabular-nums transition-colors',
                has.has(d) ? 'cursor-pointer hover:bg-shade' : 'text-muted/50',
                d === selected && 'bg-ink text-page hover:bg-ink',
                d === today && d !== selected && 'font-semibold text-s1',
              )}
            >
              {Number(d.slice(8))}
              {has.has(d) && d !== selected && <span className="absolute bottom-1 left-1/2 size-1 -translate-x-1/2 rounded-full bg-s1" />}
            </button>
          ) : (
            <div key={`e${i}`} />
          ),
        )}
      </div>
      <p className="mt-2 text-[11px] text-muted">Dot = day with data</p>
    </div>
  );
}

export const shiftDay = addDays;

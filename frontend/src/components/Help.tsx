import { ArrowUpRight, CircleHelp } from 'lucide-react';
import { HELP, type HelpKey } from '@/lib/help';
import { cn } from '@/lib/utils';
import { Popover, PopoverContent, PopoverTrigger } from './ui/popover';

/** A small (?) next to a label: what it means, in a sentence or two, and a link to the docs. */
export function Help({ topic, className }: { topic: HelpKey; className?: string }) {
  const h = HELP[topic];
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          className={cn('inline-grid size-4 shrink-0 cursor-pointer place-items-center rounded-full align-[-2px] text-muted/70 transition-colors hover:text-ink data-[state=open]:text-ink', className)}
          aria-label={`What is ${h.title}?`}
        >
          <CircleHelp className="size-3.5" />
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-72 text-[13px] leading-relaxed">
        <p className="font-medium">{h.title}</p>
        <p className="mt-1 text-ink-2">{h.text}</p>
        <a href={`/docs/${h.doc}`} target="_blank" rel="noopener" className="mt-2 inline-flex items-center gap-0.5 font-medium text-s1 hover:underline">
          Read more in the docs <ArrowUpRight className="size-3.5" />
        </a>
      </PopoverContent>
    </Popover>
  );
}

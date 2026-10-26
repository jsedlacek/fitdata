import { useEffect, useMemo, useRef, useState } from 'react';
import { Dialog } from 'radix-ui';
import { CornerDownLeft, FileText, Hash, Search as SearchIcon } from 'lucide-react';
import { navigate } from './nav';
import { PAGES } from './pages';
import { buildIndex, builtIndex, search, type Hit } from './search';
import { cn } from '@/lib/utils';

export const isMac = /Mac|iPhone|iPad/.test(navigator.platform);

type Item = { key: string; to: string; page: string; section?: string; snippet?: Hit['snippet'] };

/** The ⌘K dialog: type to search all pages; with no query, a list of pages. */
export function SearchDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const [q, setQ] = useState('');
  const [active, setActive] = useState(0);
  const list = useRef<HTMLDivElement>(null);
  const [index, setIndex] = useState(builtIndex);

  // Index while the dialog opens (in a task of its own: pages can't render inside an effect).
  useEffect(() => {
    if (!open) return setQ('');
    if (index) return;
    const id = setTimeout(() => setIndex(buildIndex()));
    return () => clearTimeout(id);
  }, [open, index]);

  const items: Item[] = useMemo(() => {
    if (!q.trim()) return PAGES.map((p) => ({ key: p.slug, to: p.slug, page: p.title, section: p.group }));
    return search(index ?? [], q).map(({ entry: e, snippet }) => ({
      key: `${e.page.slug}#${e.id}`,
      to: e.id ? `${e.page.slug}#${e.id}` : e.page.slug,
      page: e.page.title,
      section: e.id ? e.title : undefined,
      snippet,
    }));
  }, [q, index]);

  useEffect(() => setActive(0), [q]);
  useEffect(() => {
    list.current?.querySelector(`[data-i="${active}"]`)?.scrollIntoView({ block: 'nearest' });
  }, [active]);

  const go = (it: Item | undefined) => {
    if (!it) return;
    onOpenChange(false);
    navigate(it.to);
  };

  const grouped = !q.trim();
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] data-[state=open]:animate-in data-[state=open]:fade-in-0" />
        <Dialog.Content
          className="fixed left-1/2 top-[12vh] z-50 flex max-h-[70vh] w-[calc(100vw-32px)] max-w-xl -translate-x-1/2 flex-col overflow-hidden rounded-xl border bg-surface shadow-2xl outline-none data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=open]:zoom-in-95"
          aria-describedby={undefined}
        >
          <Dialog.Title className="sr-only">Search the docs</Dialog.Title>
          <div className="flex items-center gap-2.5 border-b px-4">
            <SearchIcon className="size-4 shrink-0 text-muted" />
            <input
              autoFocus
              value={q}
              onChange={(e) => setQ(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'ArrowDown') setActive((a) => Math.min(items.length - 1, a + 1));
                else if (e.key === 'ArrowUp') setActive((a) => Math.max(0, a - 1));
                else if (e.key === 'Enter') go(items[active]);
                else return;
                e.preventDefault();
              }}
              placeholder="Search the docs, e.g. HRV, deep sleep, zone minutes"
              className="h-12 min-w-0 flex-1 bg-transparent text-[15px] outline-none placeholder:text-muted"
              aria-label="Search"
              aria-controls="search-results"
              aria-activedescendant={items[active] ? `r-${active}` : undefined}
              role="combobox"
              aria-expanded
            />
            <kbd className="hidden rounded border bg-surface-2 px-1.5 text-[11px] text-muted sm:inline">esc</kbd>
          </div>
          <div ref={list} id="search-results" role="listbox" className="overflow-y-auto p-2">
            {items.length === 0 && <p className="px-3 py-10 text-center text-sm text-muted">No results for “{q}”.</p>}
            {items.map((it, i) => {
              const header = grouped && (i === 0 || PAGES[i - 1].group !== PAGES[i].group);
              return (
                <div key={it.key}>
                  {header && <div className="px-3 pb-1 pt-3 text-[11px] font-medium uppercase tracking-wider text-muted first:pt-1">{it.section}</div>}
                  <a
                    id={`r-${i}`}
                    data-i={i}
                    role="option"
                    aria-selected={i === active}
                    href={`/docs/${it.to}`}
                    onMouseMove={() => setActive(i)}
                    onClick={(e) => {
                      e.preventDefault();
                      go(it);
                    }}
                    className={cn('flex items-start gap-3 rounded-lg px-3 py-2', i === active && 'bg-shade')}
                  >
                    {it.section && !grouped ? <Hash className="mt-0.5 size-4 shrink-0 text-muted" /> : <FileText className="mt-0.5 size-4 shrink-0 text-muted" />}
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-sm">
                        {grouped ? (
                          it.page
                        ) : it.section ? (
                          <>
                            <span className="text-muted">{it.page} › </span>
                            <span className="font-medium">{it.section}</span>
                          </>
                        ) : (
                          <span className="font-medium">{it.page}</span>
                        )}
                      </span>
                      {it.snippet && (
                        <span className="mt-0.5 line-clamp-2 block text-[13px] text-ink-2">
                          {it.snippet.before}
                          {it.snippet.match && <mark className="rounded-sm bg-s1/20 px-px text-ink">{it.snippet.match}</mark>}
                          {it.snippet.after}
                        </span>
                      )}
                    </span>
                    {i === active && <CornerDownLeft className="mt-0.5 size-3.5 shrink-0 text-muted" />}
                  </a>
                </div>
              );
            })}
          </div>
          <div className="flex gap-4 border-t px-4 py-2 text-[11px] text-muted">
            <span>
              <kbd className="rounded border bg-surface-2 px-1">↑</kbd> <kbd className="rounded border bg-surface-2 px-1">↓</kbd> to move
            </span>
            <span>
              <kbd className="rounded border bg-surface-2 px-1">↵</kbd> to open
            </span>
            <span>
              <kbd className="rounded border bg-surface-2 px-1">esc</kbd> to close
            </span>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

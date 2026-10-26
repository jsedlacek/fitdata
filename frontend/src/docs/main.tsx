// The public docs at /docs: sidebar, the page, its table of contents, and ⌘K search.
import { StrictMode, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { Dialog } from 'radix-ui';
import { ArrowLeft, ArrowRight, Menu, Search, X } from 'lucide-react';
import { href, onLinkClick, scrollToHash, useSlug } from './nav';
import { GROUPS, PAGES, type Page } from './pages';
import { SearchDialog, isMac } from './Search';
import { cn } from '@/lib/utils';
import '../index.css';

function Docs() {
  const slug = useSlug();
  const page = PAGES.find((p) => p.slug === slug);
  const [searching, setSearching] = useState(false);
  const [menu, setMenu] = useState(false);

  // ⌘K / Ctrl+K toggles the search; "/" opens it unless typing somewhere.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        setSearching((s) => !s);
      } else if (e.key === '/' && !(e.target as HTMLElement).closest('input, textarea, [contenteditable]')) {
        e.preventDefault();
        setSearching(true);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  useLayoutEffect(() => {
    document.title = page ? (page.slug ? `${page.title} · Fitdata Docs` : 'Fitdata Docs') : 'Not found · Fitdata Docs';
    document.querySelector('meta[name="description"]')?.setAttribute('content', page?.summary ?? '');
    setMenu(false);
    scrollToHash();
  }, [page]);

  return (
    <div className="min-h-dvh">
      <header className="sticky top-0 z-40 border-b bg-page/85 backdrop-blur supports-[backdrop-filter]:bg-page/70">
        <div className="mx-auto flex h-14 max-w-7xl items-center gap-3 px-4">
          <button className="-ml-1.5 grid size-9 place-items-center rounded-lg text-ink-2 hover:bg-shade lg:hidden" onClick={() => setMenu(true)} aria-label="Open navigation">
            <Menu className="size-5" />
          </button>
          <a href="/docs" onClick={(e) => onLinkClick(e, '')} className="flex shrink-0 items-center gap-2 font-semibold tracking-tight">
            <img src="/favicon.svg" alt="" className="size-7" />
            Fitdata <span className="font-normal text-muted">Docs</span>
          </a>
          <div className="flex-1" />
          <button
            onClick={() => setSearching(true)}
            className="flex h-9 items-center gap-2 rounded-lg border bg-surface px-2.5 text-sm text-muted transition-colors hover:bg-shade sm:w-64"
            aria-label="Search the docs"
          >
            <Search className="size-4" />
            <span className="hidden flex-1 text-left sm:inline">Search…</span>
            <kbd className="hidden rounded border bg-surface-2 px-1.5 text-[11px] sm:inline">{isMac ? '⌘' : 'Ctrl'} K</kbd>
          </button>
          <a href="/dash" className="hidden h-9 items-center rounded-lg bg-ink px-3.5 text-sm font-medium text-page hover:opacity-90 sm:inline-flex">
            Open Fitdata
          </a>
        </div>
      </header>

      <div className="mx-auto flex max-w-7xl gap-10 px-4">
        <aside className="sticky top-14 hidden h-[calc(100dvh-56px)] w-56 shrink-0 overflow-y-auto py-8 lg:block">
          <Nav slug={slug} />
        </aside>
        <main className="min-w-0 flex-1 pb-16 pt-8">{page ? <Article key={page.slug} page={page} /> : <NotFound onSearch={() => setSearching(true)} />}</main>
      </div>

      <footer className="border-t">
        <div className="mx-auto flex max-w-7xl flex-wrap items-center gap-x-5 gap-y-2 px-4 py-6 text-[13px] text-muted">
          <a href="/" className="hover:text-ink-2">Fitdata</a>
          <a href="/privacy" className="hover:text-ink-2">Privacy</a>
          <a href="/terms" className="hover:text-ink-2">Terms</a>
          <a href="mailto:fitdata@jakub.contact" className="hover:text-ink-2">Contact</a>
          <span className="sm:ml-auto">Not affiliated with Google or Fitbit. Not a medical device.</span>
        </div>
      </footer>

      <Dialog.Root open={menu} onOpenChange={setMenu}>
        <Dialog.Portal>
          <Dialog.Overlay className="fixed inset-0 z-50 bg-black/30 data-[state=open]:animate-in data-[state=open]:fade-in-0" />
          <Dialog.Content className="fixed inset-y-0 left-0 z-50 w-72 max-w-[85vw] overflow-y-auto border-r bg-page px-4 pb-8 outline-none data-[state=open]:animate-in data-[state=open]:slide-in-from-left" aria-describedby={undefined}>
            <div className="flex h-14 items-center justify-between">
              <Dialog.Title className="font-semibold">Docs</Dialog.Title>
              <Dialog.Close className="grid size-9 place-items-center rounded-lg text-ink-2 hover:bg-shade" aria-label="Close navigation">
                <X className="size-5" />
              </Dialog.Close>
            </div>
            <Nav slug={slug} />
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
      <SearchDialog open={searching} onOpenChange={setSearching} />
    </div>
  );
}

function Nav({ slug }: { slug: string }) {
  return (
    <nav className="space-y-6 text-sm">
      {GROUPS.map((g) => (
        <div key={g}>
          <div className="mb-1.5 px-2.5 text-[11px] font-medium uppercase tracking-wider text-muted">{g}</div>
          <ul className="space-y-px">
            {PAGES.filter((p) => p.group === g).map((p) => (
              <li key={p.slug}>
                <a
                  href={href(p.slug)}
                  onClick={(e) => onLinkClick(e, p.slug)}
                  aria-current={p.slug === slug ? 'page' : undefined}
                  className={cn('block rounded-md px-2.5 py-1.5 text-ink-2 transition-colors hover:bg-shade hover:text-ink', p.slug === slug && 'bg-shade font-medium text-ink')}
                >
                  {p.title}
                </a>
              </li>
            ))}
          </ul>
        </div>
      ))}
    </nav>
  );
}

type Heading = { id: string; text: string; level: number };

function Article({ page }: { page: Page }) {
  const ref = useRef<HTMLElement>(null);
  const [headings, setHeadings] = useState<Heading[]>([]);
  const [current, setCurrent] = useState<string>('');
  const i = PAGES.indexOf(page);
  const prev = PAGES[i - 1];
  const next = PAGES[i + 1];

  useLayoutEffect(() => {
    const hs = [...ref.current!.querySelectorAll<HTMLHeadingElement>('h2[id], h3[id]')];
    setHeadings(hs.map((h) => ({ id: h.id, text: h.firstChild?.textContent ?? '', level: h.tagName === 'H2' ? 2 : 3 })));
  }, []);

  // The section being read: the last heading above the upper quarter of the window.
  useEffect(() => {
    const onScroll = () => {
      const hs = [...ref.current!.querySelectorAll<HTMLHeadingElement>('h2[id], h3[id]')];
      const line = Math.min(window.innerHeight / 4, 160);
      let cur = '';
      for (const h of hs) if (h.getBoundingClientRect().top <= line) cur = h.id;
      // At the bottom of the page the last heading may never reach the line.
      if (window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 2 && hs.length) cur = hs.at(-1)!.id;
      setCurrent(cur);
    };
    onScroll();
    window.addEventListener('scroll', onScroll, { passive: true });
    return () => window.removeEventListener('scroll', onScroll);
  }, []);

  return (
    <div className="flex gap-10">
      <article ref={ref} className="min-w-0 max-w-3xl flex-1">
        <p className="mb-2 text-[13px] font-medium text-s1">{page.group}</p>
        <h1 className="text-3xl font-semibold tracking-tight">{page.title}</h1>
        <div className="docs mt-4">
          <page.Body />
        </div>
        <nav className="mt-14 grid gap-3 border-t pt-6 sm:grid-cols-2" aria-label="Previous and next page">
          {prev ? <PageLink page={prev} dir="prev" /> : <span />}
          {next && <PageLink page={next} dir="next" />}
        </nav>
      </article>
      {headings.length > 1 && (
        <aside className="sticky top-14 hidden h-fit max-h-[calc(100dvh-56px)] w-52 shrink-0 overflow-y-auto py-1 xl:block">
          <div className="mb-2 text-[11px] font-medium uppercase tracking-wider text-muted">On this page</div>
          <ul className="space-y-1 border-l text-[13px]">
            {headings.map((h) => (
              <li key={h.id}>
                <a
                  href={`#${h.id}`}
                  onClick={(e) => onLinkClick(e, `#${h.id}`)}
                  className={cn(
                    '-ml-px block border-l py-0.5 text-ink-2 transition-colors hover:text-ink',
                    h.level === 3 ? 'pl-6' : 'pl-3',
                    current === h.id ? 'border-s1 font-medium text-ink' : 'border-transparent',
                  )}
                >
                  {h.text}
                </a>
              </li>
            ))}
          </ul>
        </aside>
      )}
    </div>
  );
}

function PageLink({ page, dir }: { page: Page; dir: 'prev' | 'next' }) {
  return (
    <a
      href={href(page.slug)}
      onClick={(e) => onLinkClick(e, page.slug)}
      className={cn('group rounded-xl border p-4 transition-colors hover:bg-shade', dir === 'next' && 'text-right sm:col-start-2')}
    >
      <span className={cn('flex items-center gap-1 text-xs text-muted', dir === 'next' && 'justify-end')}>
        {dir === 'prev' ? (
          <>
            <ArrowLeft className="size-3.5" /> Previous
          </>
        ) : (
          <>
            Next <ArrowRight className="size-3.5" />
          </>
        )}
      </span>
      <span className="mt-1 block font-medium group-hover:text-s1">{page.title}</span>
    </a>
  );
}

function NotFound({ onSearch }: { onSearch: () => void }) {
  return (
    <div className="py-20 text-center">
      <h1 className="text-2xl font-semibold tracking-tight">Page not found</h1>
      <p className="mt-2 text-ink-2">This page of the docs doesn't exist, or has moved.</p>
      <div className="mt-6 flex justify-center gap-3">
        <button onClick={onSearch} className="inline-flex h-9 items-center rounded-lg border bg-surface px-3.5 text-sm font-medium hover:bg-shade">
          Search the docs
        </button>
        <a href="/docs" onClick={(e) => onLinkClick(e, '')} className="inline-flex h-9 items-center rounded-lg bg-ink px-3.5 text-sm font-medium text-page hover:opacity-90">
          Go to the start
        </a>
      </div>
    </div>
  );
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Docs />
  </StrictMode>,
);

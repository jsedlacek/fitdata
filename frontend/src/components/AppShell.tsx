import { Link, Outlet } from '@tanstack/react-router';
import { useEffect, useReducer, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { BookOpen, Bot, CalendarDays, ChartSpline, CheckCircle2, RefreshCw, Settings, TriangleAlert } from 'lucide-react';
import { overviewQuery, saveSettings, startSync, syncQuery, type Me, type Overview, type SyncRun } from '@/lib/api';
import { DEMO } from '@/lib/demo';
import { detectSettings, setPrefs } from '@/lib/prefs';
import { ago } from '@/lib/format';
import { cn } from '@/lib/utils';
import { Button } from './ui/button';
import { ReconnectBanner } from './ReconnectBanner';
import { Popover, PopoverContent, PopoverTrigger } from './ui/popover';
import { McpConnect } from './McpConnect';

export function AppShell() {
  const { data } = useQuery(overviewQuery);
  // Before the pages render: the formatting helpers read the settings.
  if (data) setPrefs(data.user.settings);
  useDetectedSettings(data?.user);
  return (
    <div className="min-h-dvh">
      {/* Phones have the tab bar at the bottom instead, and the page starts with its title. */}
      <header className="sticky top-0 z-40 hidden border-b bg-page/85 backdrop-blur supports-[backdrop-filter]:bg-page/70 sm:block">
        <div className="mx-auto flex h-14 max-w-6xl items-center gap-6 px-6">
          <Link to="/" search={{}} className="flex shrink-0 items-center gap-2 font-semibold tracking-tight">
            <img src="/favicon.svg" alt="" className="size-7" />
            <span>Fitdata</span>
          </Link>
          <nav className="flex items-center gap-1 text-sm">
            <NavLink to="/">Day</NavLink>
            <NavLink to="/trends">Trends</NavLink>
          </nav>
          <div className="flex-1" />
          {data && <AskAi />}
          {!DEMO && data?.user.connected !== false && <SyncStatus run={data?.lastSync ?? null} />}
          <Button asChild variant="ghost" size="icon-sm" className="text-ink-2 hover:text-ink">
            <a href="/docs" target="_blank" rel="noopener" aria-label="Docs: what the metrics and charts mean" title="Docs: what the metrics and charts mean">
              <BookOpen />
            </a>
          </Button>
          {data && (
            <Button asChild variant="ghost" size="icon-sm" className="text-ink-2 hover:text-ink">
              <Link to="/settings" aria-label="Settings" title="Settings" activeProps={{ className: 'bg-shade !text-ink' }}>
                <Settings />
              </Link>
            </Button>
          )}
        </div>
      </header>
      <main className="mx-auto max-w-6xl px-5 pb-16 pt-[max(env(safe-area-inset-top),1.5rem)] sm:px-6 sm:pt-5">
        {DEMO ? <DemoNote /> : <ReconnectBanner run={data?.lastSync ?? null} user={data?.user ?? null} />}
        <Outlet />
        {DEMO && <DemoAbout />}
      </main>
      {data && (
        <footer className="mx-auto flex max-w-6xl flex-wrap items-center gap-x-5 gap-y-2 px-5 pb-28 text-[13px] text-muted sm:px-6 sm:pb-8">
          <span className="flex gap-4 sm:ml-auto">
            <a href="/docs" className="hover:text-ink-2">Docs</a>
            <a href="/privacy" className="hover:text-ink-2">Privacy</a>
            <a href="/terms" className="hover:text-ink-2">Terms</a>
            <a href="https://github.com/jsedlacek/fitdata" className="hover:text-ink-2">GitHub</a>
          </span>
        </footer>
      )}
      {data && <TabBar failed={!!data.lastSync && !data.lastSync.ok} />}
    </div>
  );
}

/**
 * The demo's short note: what the data is, and the way to your own. Tinted
 * like a test-mode strip, so it never reads as someone's real dashboard.
 */
function DemoNote() {
  return (
    <div className="mb-6 flex flex-wrap items-center gap-x-4 gap-y-2 rounded-xl bg-[color-mix(in_srgb,var(--s1)_9%,var(--surface))] px-4 py-3 text-[13px]">
      <p className="min-w-0 flex-1 basis-72 text-ink-2">
        <span className="mr-1.5 rounded-md bg-s1 px-1.5 py-0.5 text-[11px] font-semibold uppercase tracking-wide text-white">Demo</span>
        90 days of a made-up person&apos;s Fitbit data, generated in your browser.
      </p>
      <div className="flex items-center gap-4">
        <a href="#about-demo" className="text-ink-2 underline-offset-2 hover:text-ink hover:underline">
          How it works
        </a>
        <a href="/auth/login" className="inline-flex h-8 items-center rounded-lg bg-ink px-3 font-medium text-page hover:opacity-90">
          Connect your Fitbit
        </a>
      </div>
    </div>
  );
}

/** The longer version at the end of every demo page: what this is, why, and what connecting your own data does. */
function DemoAbout() {
  const steps = [
    ['Sign in with Google', 'and allow Fitdata to read your Fitbit data, through Google’s official Health API. Read-only: it can’t change anything.'],
    ['Your history loads', 'in a few minutes, the same pages as here, and stays up to date every hour.'],
    ['Ask your AI', 'if you like: add Fitdata to Claude or ChatGPT and ask about your own nights and days.'],
  ];
  return (
    <section id="about-demo" className="mt-16 scroll-mt-20 rounded-2xl border bg-surface p-6 sm:p-8" aria-labelledby="about-demo-title">
      <h2 id="about-demo-title" className="text-xl font-semibold tracking-tight">
        This is a demo
      </h2>
      <div className="mt-6 grid gap-8 md:grid-cols-3">
        <div>
          <h3 className="text-sm font-semibold">What you’re seeing</h3>
          <p className="mt-1.5 text-sm text-ink-2">
            90 days of a made-up person: their nights, heart rate, HRV, SpO₂, steps and workouts, generated in your browser to look like real Fitbit
            data. It isn’t anyone’s real data, and nothing you do here is stored.
          </p>
        </div>
        <div>
          <h3 className="text-sm font-semibold">Why a demo</h3>
          <p className="mt-1.5 text-sm text-ink-2">
            So you can try every page before you connect anything: hover a chart to read all rows at one moment, step through the days, open Trends.
          </p>
        </div>
        <div>
          <h3 className="text-sm font-semibold">With your own data</h3>
          <ol className="mt-1.5 space-y-2 text-sm text-ink-2">
            {steps.map(([title, text], i) => (
              <li key={title} className="flex gap-2.5">
                <span className="mt-0.5 grid size-5 shrink-0 place-items-center rounded-full bg-ink text-[11px] font-semibold text-page">{i + 1}</span>
                <span>
                  <b className="font-medium text-ink">{title}</b> {text}
                </span>
              </li>
            ))}
          </ol>
        </div>
      </div>
      <div className="mt-8 flex flex-wrap items-center justify-between gap-x-6 gap-y-4 border-t pt-6">
        <ul className="flex flex-wrap gap-x-5 gap-y-1.5 text-[13px] text-ink-2">
          {['Read-only', 'Delete everything in one click', 'Stored in the EU', 'Free and open source'].map((t) => (
            <li key={t} className="flex items-center gap-1.5">
              <CheckCircle2 className="size-3.5 text-good" />
              {t}
            </li>
          ))}
        </ul>
        <div className="flex flex-wrap items-center gap-3">
          <a href="/docs" className="text-[13px] text-ink-2 underline-offset-2 hover:text-ink hover:underline">
            Read the docs
          </a>
          <a href="/auth/login" className="inline-flex h-10 items-center rounded-lg bg-ink px-4 text-sm font-medium text-page hover:opacity-90">
            Connect your Fitbit
          </a>
        </div>
      </div>
    </section>
  );
}

/** Phones: the pages and the assistant at the bottom of the screen, as in native apps. */
function TabBar({ failed }: { failed: boolean }) {
  const item = 'flex flex-col items-center gap-0.5 pt-1.5 text-[11px] text-ink-2';
  const pill = 'grid h-7 w-14 place-items-center rounded-full transition-colors [&_svg]:size-[18px]';
  const tab = (to: '/' | '/trends' | '/settings', label: string, Icon: typeof Settings, dot?: boolean) => (
    <Link
      to={to}
      search={{}}
      className={item}
      activeOptions={{ exact: true, includeSearch: false }}
      activeProps={{ className: '!text-ink font-medium [&>span]:bg-[color-mix(in_srgb,var(--s1)_14%,transparent)] [&>span]:text-s1' }}
    >
      <span className={cn(pill, 'relative')}>
        <Icon />
        {dot && <span className="absolute right-3 top-0.5 size-2 rounded-full bg-critical ring-2 ring-page" />}
      </span>
      {label}
    </Link>
  );
  return (
    <nav
      aria-label="Pages"
      className="fixed inset-x-0 bottom-0 z-40 grid grid-cols-4 border-t bg-page/90 px-3 pb-[max(env(safe-area-inset-bottom),0.75rem)] backdrop-blur sm:hidden"
    >
      {tab('/', 'Day', CalendarDays)}
      {tab('/trends', 'Trends', ChartSpline)}
      <AskAi tab className={item} pillClassName={pill} />
      {/* The sync control is in Settings on phones; a failed sync marks the tab. */}
      {tab('/settings', 'Settings', Settings, failed)}
    </nav>
  );
}

/** A new user's first visit saves settings from the browser: time zone, units, week start, clock. */
function useDetectedSettings(user: Me | undefined) {
  const queryClient = useQueryClient();
  const sent = useRef(false);
  useEffect(() => {
    if (!user || user.settingsSaved || sent.current) return;
    sent.current = true;
    saveSettings(detectSettings())
      .then((me) => queryClient.setQueryData<Overview>(overviewQuery.queryKey, (o) => o && { ...o, user: me }))
      .catch(() => {
        // Next visit tries again; the defaults show meanwhile.
      });
  }, [user, queryClient]);
}

const MCP_SEEN = 'mcp-seen';

/**
 * Asking Claude or ChatGPT is what sets Fitdata apart, so the header offers
 * it; a dot marks it until the instructions have been opened once.
 */
function AskAi({ tab, className, pillClassName }: { tab?: boolean; className?: string; pillClassName?: string }) {
  const [seen, setSeen] = useState(() => {
    try {
      return localStorage.getItem(MCP_SEEN) === '1';
    } catch {
      return true;
    }
  });
  const open = (isOpen: boolean) => {
    if (!isOpen || seen) return;
    setSeen(true);
    try {
      localStorage.setItem(MCP_SEEN, '1');
    } catch {
      // Private mode: the dot comes back next time, nothing worse.
    }
  };
  return (
    <Popover onOpenChange={open}>
      <PopoverTrigger asChild>
        {tab ? (
          <button className={className} aria-label="Ask Claude or ChatGPT about your data">
            <span className={cn(pillClassName, 'relative')}>
              <Bot />
              {!seen && <span className="absolute right-3 top-0.5 size-2 rounded-full bg-s1 ring-2 ring-page" />}
            </span>
            Ask AI
          </button>
        ) : (
        <Button variant="ghost" size="sm" className="px-2 text-ink-2 hover:text-ink md:px-2.5" aria-label="Ask Claude or ChatGPT about your data">
          <span className="relative">
            <Bot />
            {!seen && <span className="absolute -right-1 -top-1 size-2 rounded-full bg-s1 ring-2 ring-page" />}
          </span>
          <span className="hidden md:inline">Ask Claude or ChatGPT</span>
        </Button>
        )}
      </PopoverTrigger>
      <PopoverContent align="end" side={tab ? 'top' : 'bottom'} className="w-80 max-w-[calc(100vw-2rem)]">
        <McpConnect />
      </PopoverContent>
    </Popover>
  );
}

function NavLink({ to, children }: { to: '/' | '/trends'; children: React.ReactNode }) {
  return (
    <Link
      to={to}
      className="rounded-md px-2 py-1.5 text-ink-2 transition-colors sm:px-2.5 hover:bg-shade hover:text-ink"
      activeProps={{ className: 'bg-shade !text-ink font-medium' }}
      activeOptions={{ exact: true, includeSearch: false }}
    >
      {children}
    </Link>
  );
}

export function SyncStatus({ run, labelled }: { run: SyncRun | null; labelled?: boolean }) {
  const queryClient = useQueryClient();
  const sync = useQuery(syncQuery);
  const start = useMutation({
    mutationFn: startSync,
    onSuccess: (state) => queryClient.setQueryData(syncQuery.queryKey, state),
  });
  const running = start.isPending || !!sync.data?.running;

  // When a run (ours or the scheduled one) finishes, reload the data.
  const wasRunning = useRef(false);
  useEffect(() => {
    if (wasRunning.current && !running) {
      void queryClient.invalidateQueries({ predicate: (q) => q.queryKey[0] !== 'sync' });
    }
    wasRunning.current = running;
  }, [running, queryClient]);

  // Re-render every minute so "5 min ago" keeps counting.
  const [, tick] = useReducer((n: number) => n + 1, 0);
  useEffect(() => {
    const id = setInterval(tick, 60_000);
    return () => clearInterval(id);
  }, []);

  // One control: the state of the last sync, and a click starts a new one.
  const failed = start.isError || (run && !run.ok);
  const [Icon, color] = running
    ? [RefreshCw, '']
    : failed
      ? [TriangleAlert, 'text-critical']
      : run
        ? [CheckCircle2, 'text-good']
        : [RefreshCw, ''];
  const label = running ? 'Syncing…' : start.isError ? 'Could not start the sync' : run ? (run.ok ? 'Synced' : 'Sync failed') : 'Sync now';
  const title = running
    ? 'Sync in progress'
    : `${run ? `Last sync ${new Date(run.run_at).toLocaleString('en-GB')}. ` : ''}Click to sync now`;
  return (
    <Button
      variant="ghost"
      size="sm"
      className="group px-2 text-[13px] font-normal text-ink-2 hover:text-ink disabled:opacity-100 md:px-2.5"
      disabled={running}
      onClick={() => start.mutate()}
      aria-label={running ? 'Syncing' : 'Sync now'}
      title={title}
    >
      {/* At rest the icon tells how the last sync went; on hover it offers a new one. */}
      <Icon className={cn(color, running && 'animate-spin', !running && 'group-hover:hidden')} />
      {!running && <RefreshCw className="hidden group-hover:block" />}
      <span className={labelled ? '' : 'hidden md:inline'}>{label}</span>
      {run && !running && !start.isError && <span className={cn('text-muted', !labelled && 'hidden md:inline')}>{ago(run.run_at)}</span>}
    </Button>
  );
}



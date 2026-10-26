import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { BookOpen, Check, Copy, Download, Laptop, LifeBuoy, LogOut, Moon, Sun, Trash2 } from 'lucide-react';
import {
  deleteAccount,
  EXPORT_URL,
  overviewQuery,
  saveSettings,
  syncQuery,
  type Clock,
  type Overview,
  type Settings,
  type Units,
  type WeekStart,
} from '@/lib/api';
import { DEMO } from '@/lib/demo';
import { browserTimeZone, timeZones } from '@/lib/prefs';
import { SyncStatus } from '@/components/AppShell';
import { InstallCard } from '@/components/InstallApp';
import { Button } from '@/components/ui/button';
import { Card, CardBody, CardHeader } from '@/components/ui/card';
import { Segmented } from '@/components/ui/segmented';
import { Skeleton } from '@/components/ui/skeleton';
import { useThemeChoice, type ThemeChoice } from '@/lib/theme';
import { ErrorCard } from './day';

/** Where questions and privacy requests go (`CONTACT_EMAIL` in src/web.rs). */
const SUPPORT_EMAIL = 'fitdata@jakub.contact';

export function SettingsPage() {
  const overview = useQuery(overviewQuery);
  if (overview.isError) return <ErrorCard onRetry={() => overview.refetch()} />;
  const user = overview.data?.user;
  return (
    <div className="mx-auto max-w-2xl space-y-5">
      <div className="flex items-center justify-between gap-3">
        <h1 className="text-xl font-semibold tracking-tight">Settings</h1>
        {!DEMO && (
        <Button asChild variant="outline" size="sm">
          <a href="/auth/logout">
            <LogOut /> Sign out
          </a>
        </Button>
        )}
      </div>
      {user ? (
        <>
          {user.connected && !DEMO && (
            // Phones have no header, so the sync control lives here.
            <Card className="sm:hidden">
              <CardHeader title="Sync" subtitle="Fitdata syncs your Fitbit every hour. Tap to sync now." action={<SyncStatus run={overview.data?.lastSync ?? null} labelled />} />
            </Card>
          )}
          <Preferences settings={user.settings} connected={user.connected} />
          {!DEMO && <InstallCard />}
          {!DEMO && (
            <>
              <YourData />
              <Support userId={user.id} />
              <DeleteAccount />
            </>
          )}
        </>
      ) : (
        Array.from({ length: 3 }, (_, i) => <Skeleton key={i} className="h-48 rounded-xl" />)
      )}
    </div>
  );
}

function Preferences({ settings, connected }: { settings: Settings; connected: boolean }) {
  const queryClient = useQueryClient();
  const [movedTz, setMovedTz] = useState(false);
  const save = useMutation({
    mutationFn: saveSettings,
    onMutate: (s) => {
      // Show the choice at once; the server's answer confirms it.
      queryClient.setQueryData<Overview>(overviewQuery.queryKey, (o) => o && { ...o, user: { ...o.user, settings: s } });
      return { moved: s.timeZone !== settings.timeZone };
    },
    onSuccess: (user, _s, ctx) => {
      queryClient.setQueryData<Overview>(overviewQuery.queryKey, (o) => o && { ...o, user });
      if (ctx.moved) {
        setMovedTz(true);
        void queryClient.invalidateQueries({ queryKey: syncQuery.queryKey });
      }
    },
    onError: () => void queryClient.invalidateQueries({ queryKey: overviewQuery.queryKey }),
  });
  const set = (change: Partial<Settings>) => save.mutate({ ...settings, ...change });
  const detected = browserTimeZone();

  return (
    <Card>
      <CardHeader
        title="Preferences"
        subtitle="How dates, times and values are shown here and to Claude or ChatGPT."
        action={<SaveState pending={save.isPending} error={save.isError} saved={save.isSuccess} />}
      />
      <CardBody className="divide-y">
        <Row label="Time zone" hint="Days start at midnight in this time zone.">
          <div className="flex flex-col items-stretch gap-1.5 sm:items-end">
            <select
              value={settings.timeZone}
              onChange={(e) => set({ timeZone: e.target.value })}
              aria-label="Time zone"
              className="h-8 w-full rounded-lg border bg-surface px-2 text-[13px] sm:w-60"
            >
              {withCurrent(timeZones(), settings.timeZone).map((tz) => (
                <option key={tz} value={tz}>
                  {tz.replaceAll('_', ' ')}
                </option>
              ))}
            </select>
            {detected !== settings.timeZone && (
              <button className="cursor-pointer text-left text-xs text-s1 hover:underline sm:text-right" onClick={() => set({ timeZone: detected })}>
                Use this device's time zone ({detected.replaceAll('_', ' ')})
              </button>
            )}
          </div>
        </Row>
        {movedTz && (
          <p className="py-3 text-[13px] text-ink-2">
            {connected
              ? 'Your data is being recalculated for the new time zone. This takes a few minutes; the sync icon spins until it is done.'
              : 'Your data will follow the new time zone once Fitbit is connected.'}
          </p>
        )}
        <Row label="Units" hint="For temperatures here, and for every value Claude or ChatGPT tell you.">
          <Segmented<Units>
            label="Units"
            value={settings.units}
            onChange={(units) => set({ units })}
            options={[
              { value: 'metric', label: 'Metric (°C, km)' },
              { value: 'imperial', label: 'Imperial (°F, mi)' },
            ]}
          />
        </Row>
        <Row label="Week starts on" hint="In the calendar, and for Claude or ChatGPT.">
          <Segmented<WeekStart>
            label="Week starts on"
            value={settings.weekStart}
            onChange={(weekStart) => set({ weekStart })}
            options={[
              { value: 'monday', label: 'Monday' },
              { value: 'sunday', label: 'Sunday' },
              { value: 'saturday', label: 'Saturday' },
            ]}
          />
        </Row>
        <Row label="Time format">
          <Segmented<Clock>
            label="Time format"
            value={settings.clock}
            onChange={(clock) => set({ clock })}
            options={[
              { value: '24h', label: '24-hour (18:30)' },
              { value: '12h', label: '12-hour (6:30 pm)' },
            ]}
          />
        </Row>
        <Row label="Theme" hint="Saved in this browser.">
          <ThemeToggle />
        </Row>
      </CardBody>
    </Card>
  );
}

function ThemeToggle() {
  const [choice, setChoice] = useThemeChoice();
  return (
    <Segmented<ThemeChoice>
      label="Theme"
      value={choice}
      onChange={setChoice}
      options={[
        { value: 'light', label: <><Sun /> Light</> },
        { value: 'system', label: <><Laptop /> System</> },
        { value: 'dark', label: <><Moon /> Dark</> },
      ]}
    />
  );
}

/** The saved zone may be an alias the browser lists under another name. */
const withCurrent = (zones: string[], current: string) => (zones.includes(current) ? zones : [current, ...zones]);

function SaveState({ pending, error, saved }: { pending: boolean; error: boolean; saved: boolean }) {
  if (pending) return <span className="text-[13px] text-muted">Saving…</span>;
  if (error) return <span className="text-[13px] text-critical">Could not save, try again</span>;
  if (saved)
    return (
      <span className="inline-flex items-center gap-1 text-[13px] text-good">
        <Check className="size-3.5" /> Saved
      </span>
    );
  return null;
}

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-2 py-3 first:pt-0 last:pb-0 sm:flex-row sm:items-center sm:justify-between sm:gap-6">
      <div className="min-w-0">
        <div className="text-sm font-medium">{label}</div>
        {hint && <div className="text-[13px] text-muted">{hint}</div>}
      </div>
      <div className="shrink-0 overflow-x-auto">{children}</div>
    </div>
  );
}

function YourData() {
  return (
    <Card>
      <CardHeader title="Your data" subtitle="A copy of everything Fitdata stores about you." />
      <CardBody className="space-y-3">
        <p className="text-sm text-ink-2">
          One ZIP file with your account and settings, your data exactly as Google Health returned it (JSON) and the
          tables the dashboard reads (Parquet). A long history may take a minute to download.
        </p>
        <Button asChild variant="outline" size="sm">
          <a href={EXPORT_URL} download>
            <Download /> Download your data
          </a>
        </Button>
      </CardBody>
    </Card>
  );
}

function Support({ userId }: { userId: string }) {
  const [copied, setCopied] = useState(false);
  const mailto = `mailto:${SUPPORT_EMAIL}?subject=${encodeURIComponent('Fitdata support')}&body=${encodeURIComponent(
    `\n\n---\nFitdata user ID: ${userId}`,
  )}`;
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(userId);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // No clipboard access: the ID is on screen to copy by hand.
    }
  };
  return (
    <Card>
      <CardHeader title="Help" subtitle="Questions, problems, ideas or privacy requests." />
      <CardBody className="space-y-3">
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <Button asChild variant="outline" size="sm">
            <a href="/docs" target="_blank" rel="noopener">
              <BookOpen /> Read the docs
            </a>
          </Button>
          <Button asChild variant="outline" size="sm">
            <a href={mailto}>
              <LifeBuoy /> Contact support
            </a>
          </Button>
          <a href={mailto} className="text-sm text-ink-2 hover:text-ink">
            {SUPPORT_EMAIL}
          </a>
        </div>
        <div className="flex flex-wrap items-center gap-2 text-[13px] text-muted">
          <span>Your user ID</span>
          <code className="rounded bg-surface-2 px-1.5 py-0.5 text-ink-2">{userId}</code>
          <button className="inline-flex cursor-pointer items-center gap-1 hover:text-ink" onClick={copy} aria-label="Copy user ID">
            {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
            {copied ? 'Copied' : 'Copy'}
          </button>
        </div>
        <p className="text-[13px] text-muted">
          Fitdata never learns your name or e-mail, so mention the user ID when you write from another device.
        </p>
      </CardBody>
    </Card>
  );
}

function DeleteAccount() {
  const [confirming, setConfirming] = useState(false);
  const remove = useMutation({ mutationFn: deleteAccount });
  return (
    <Card className="border-l-4 border-l-critical">
      <CardHeader title="Delete account" subtitle="Fitdata stops reading your Fitbit and removes all downloaded data." />
      <CardBody>
        {confirming ? (
          <div className="space-y-2">
            <p className="text-sm">Delete your account and all downloaded data? This cannot be undone.</p>
            <div className="flex gap-2">
              <Button size="sm" className="bg-critical text-white" disabled={remove.isPending} onClick={() => remove.mutate()}>
                {remove.isPending ? 'Deleting…' : 'Delete everything'}
              </Button>
              <Button variant="outline" size="sm" onClick={() => setConfirming(false)}>
                Cancel
              </Button>
            </div>
            {remove.isError && <p className="text-sm text-critical">Could not delete the account, try again.</p>}
          </div>
        ) : (
          <Button variant="outline" size="sm" className="text-critical" onClick={() => setConfirming(true)}>
            <Trash2 /> Delete account
          </Button>
        )}
      </CardBody>
    </Card>
  );
}

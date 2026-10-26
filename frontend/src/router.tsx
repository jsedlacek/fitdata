import { createRootRoute, createRoute, createRouter } from '@tanstack/react-router';
import { AppShell } from '@/components/AppShell';
import { DayPage } from '@/routes/day';
import { SettingsPage } from '@/routes/settings';
import { TrendsPage, type Period } from '@/routes/trends';

const rootRoute = createRootRoute({ component: AppShell });

const dayRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  validateSearch: (s: Record<string, unknown>): { date?: string } =>
    typeof s.date === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(s.date) ? { date: s.date } : {},
  component: function Day() {
    const { date } = dayRoute.useSearch();
    return <DayPage date={date} />;
  },
});

const RANGES: Period[] = [7, 30, 90, 'all'];

const trendsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/trends',
  validateSearch: (s: Record<string, unknown>): { range?: Period } => {
    // Old links have the number as a string (`?range="90"`).
    const r = s.range === 'all' ? 'all' : Number(s.range);
    return RANGES.includes(r as Period) ? { range: r as Period } : {};
  },
  component: function Trends() {
    const { range } = trendsRoute.useSearch();
    return <TrendsPage range={range ?? 30} />;
  },
});

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/settings',
  component: SettingsPage,
});

export const router = createRouter({
  routeTree: rootRoute.addChildren([dayRoute, trendsRoute, settingsRoute]),
  defaultPreload: 'intent',
});

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router;
  }
}

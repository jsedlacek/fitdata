// A tiny router for the docs: `/docs/<slug>#<section>`, history API, no dependencies.
import { useSyncExternalStore, type MouseEvent } from 'react';

export const BASE = '/docs';

const listeners = new Set<() => void>();
const notify = () => listeners.forEach((l) => l());
window.addEventListener('popstate', notify);

/** The current page's slug ('' for the start page). */
export const currentSlug = () => location.pathname.replace(/^\/docs\/?/, '').replace(/\/$/, '');

export function useSlug() {
  return useSyncExternalStore(
    (cb) => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    currentSlug,
  );
}

/** "hrv#reading" → "/docs/hrv#reading"; "#reading" stays on the current page. */
export const href = (to: string) => (to.startsWith('#') ? `${BASE}/${currentSlug()}${to}`.replace(/\/#/, '#') : `${BASE}${to ? `/${to}` : ''}`);

/** Scrolls to the section in the URL's hash, or to the top. */
export function scrollToHash(smooth = false) {
  const id = decodeURIComponent(location.hash.slice(1));
  const el = id ? document.getElementById(id) : null;
  if (el) el.scrollIntoView({ behavior: smooth ? 'smooth' : 'auto', block: 'start' });
  else window.scrollTo({ top: 0 });
}

export function navigate(to: string) {
  const url = href(to);
  const samePage = url.split('#')[0] === location.pathname;
  history.pushState(null, '', url);
  if (samePage) scrollToHash(true);
  else notify();
}

/** Click handler that keeps plain clicks in the app and leaves modified clicks to the browser. */
export function onLinkClick(e: MouseEvent<HTMLAnchorElement>, to: string) {
  if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
  e.preventDefault();
  navigate(to);
}

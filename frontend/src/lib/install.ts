// Installing Fitdata as an app (PWA). Chrome and Edge offer a prompt we can trigger from
// a button; Safari on iPhone and iPad only has Share → Add to Home Screen.
import { useSyncExternalStore } from 'react';

type PromptEvent = Event & { prompt: () => Promise<void>; userChoice: Promise<{ outcome: 'accepted' | 'dismissed' }> };

let deferred: PromptEvent | null = null;
const subs = new Set<() => void>();
const notify = () => subs.forEach((f) => f());

// The browser fires this once, early: keep it until a button asks for it.
window.addEventListener('beforeinstallprompt', (e) => {
  e.preventDefault();
  deferred = e as PromptEvent;
  notify();
});
window.addEventListener('appinstalled', () => {
  deferred = null;
  notify();
});

const standalone = () => matchMedia('(display-mode: standalone)').matches || (navigator as { standalone?: boolean }).standalone === true;
/** iPhone or iPad Safari (iPadOS says it is a Mac, but has touch). */
const ios = () => /iphone|ipad|ipod/i.test(navigator.userAgent) || (navigator.userAgent.includes('Macintosh') && navigator.maxTouchPoints > 1);

/** How Fitdata can be installed here: already installed, a prompt, iOS steps, or not at all. */
export type InstallState = 'installed' | 'prompt' | 'ios' | 'none';

function state(): InstallState {
  if (standalone()) return 'installed';
  if (deferred) return 'prompt';
  if (ios()) return 'ios';
  return 'none';
}

export function useInstall() {
  const s = useSyncExternalStore((f) => {
    subs.add(f);
    return () => void subs.delete(f);
  }, state);
  const install = async () => {
    if (!deferred) return;
    await deferred.prompt();
    await deferred.userChoice;
    deferred = null;
    notify();
  };
  return { state: s, install };
}

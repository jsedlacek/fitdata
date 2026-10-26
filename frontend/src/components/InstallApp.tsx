import { useState } from 'react';
import { Download, Share, SquarePlus, X } from 'lucide-react';
import { useInstall } from '@/lib/install';
import { Button } from './ui/button';
import { Card, CardHeader } from './ui/card';

/** Settings: install Fitdata as an app, with the browser's prompt or, on iPhone and iPad, the two Safari steps. */
export function InstallCard() {
  const { state, install } = useInstall();
  if (state === 'installed' || state === 'none') return null;
  return (
    <Card>
      <CardHeader
        title="Install the app"
        subtitle="Fitdata on your home screen, opening full screen like any other app."
        action={
          state === 'prompt' ? (
            <Button size="sm" onClick={() => void install()}>
              <Download /> Install
            </Button>
          ) : undefined
        }
      />
      {state === 'ios' && <IosSteps className="px-5 pb-5" />}
    </Card>
  );
}

function IosSteps({ className }: { className?: string }) {
  return (
    <ol className={`space-y-1.5 text-[13px] text-ink-2 ${className ?? ''}`}>
      <li>
        1. Tap <Share className="mx-0.5 inline size-4 align-[-3px] text-s1" aria-label="Share" /> Share in Safari&apos;s toolbar.
      </li>
      <li>
        2. Choose <SquarePlus className="mx-0.5 inline size-4 align-[-3px]" aria-hidden /> <b className="font-medium text-ink">Add to Home Screen</b>.
      </li>
    </ol>
  );
}

const DISMISSED = 'install-dismissed';

/** Phones, once: a quiet offer to install, until it is installed or dismissed. */
export function InstallHint() {
  const { state, install } = useInstall();
  const [hidden, setHidden] = useState(() => {
    try {
      return localStorage.getItem(DISMISSED) === '1';
    } catch {
      return false;
    }
  });
  if (hidden || state === 'installed' || state === 'none') return null;
  const dismiss = () => {
    setHidden(true);
    try {
      localStorage.setItem(DISMISSED, '1');
    } catch {
      // Private mode: it comes back next time, nothing worse.
    }
  };
  return (
    <div className="mb-6 flex items-start gap-3 rounded-xl bg-surface-2 px-4 py-3 text-[13px] sm:hidden">
      <img src="/icon-192.png" alt="" className="size-9 shrink-0 rounded-lg" />
      <div className="min-w-0 flex-1">
        <p className="font-medium text-ink">Add Fitdata to your home screen</p>
        {state === 'prompt' ? (
          <button className="mt-1 font-medium text-s1" onClick={() => void install()}>
            Install
          </button>
        ) : (
          <IosSteps className="mt-1" />
        )}
      </div>
      <button className="-mr-1 grid size-7 place-items-center rounded-full text-muted hover:bg-shade" onClick={dismiss} aria-label="Not now">
        <X className="size-4" />
      </button>
    </div>
  );
}

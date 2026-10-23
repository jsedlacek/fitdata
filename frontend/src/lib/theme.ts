import { useEffect, useState, useSyncExternalStore } from 'react';

export type ThemeChoice = 'system' | 'light' | 'dark';

const media = () => window.matchMedia('(prefers-color-scheme: dark)');

function stored(): ThemeChoice {
  try {
    const t = localStorage.getItem('theme');
    return t === 'light' || t === 'dark' ? t : 'system';
  } catch {
    return 'system';
  }
}

export function useThemeChoice() {
  const [choice, setChoice] = useState<ThemeChoice>(stored);
  useEffect(() => {
    const root = document.documentElement;
    if (choice === 'system') delete root.dataset.theme;
    else root.dataset.theme = choice;
    try {
      if (choice === 'system') localStorage.removeItem('theme');
      else localStorage.setItem('theme', choice);
    } catch {
      /* private mode: theme just won't persist */
    }
    window.dispatchEvent(new Event('themechange'));
  }, [choice]);
  return [choice, setChoice] as const;
}

/** The theme actually shown; charts re-read CSS variables when it changes. */
export function useResolvedTheme(): 'light' | 'dark' {
  return useSyncExternalStore(
    (cb) => {
      const m = media();
      m.addEventListener('change', cb);
      window.addEventListener('themechange', cb);
      return () => {
        m.removeEventListener('change', cb);
        window.removeEventListener('themechange', cb);
      };
    },
    () => {
      const t = document.documentElement.dataset.theme;
      if (t === 'light' || t === 'dark') return t;
      return media().matches ? 'dark' : 'light';
    },
  );
}

export const cssVar = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

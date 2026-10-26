import { useSyncExternalStore } from 'react';

/** A value outside React: only components that read it re-render when it changes. */
export function createStore<T>(init: T) {
  let v = init;
  const subs = new Set<() => void>();
  return {
    get: () => v,
    set: (n: T) => {
      if (Object.is(n, v)) return;
      v = n;
      subs.forEach((f) => f());
    },
    subscribe: (f: () => void) => {
      subs.add(f);
      return () => void subs.delete(f);
    },
  };
}

export type Store<T> = ReturnType<typeof createStore<T>>;

export const useStore = <T,>(s: Store<T>) => useSyncExternalStore(s.subscribe, s.get);

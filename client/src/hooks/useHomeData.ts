/** Displays a cached Home snapshot while refreshing it in the background. */
import { useCallback, useEffect, useRef, useSyncExternalStore } from 'react';
import { homeSnapshot, onHomeInvalidated, refreshHomeData, subscribeHome } from '../homeCache';

/** Refresh on mount, explicit changes, mutations, and return from an idle tab. */
export function useHomeData<T>(key: string, load: () => Promise<T>, fallback: T, refreshKey = 0, enabled = true) {
  const loader = useRef(load);
  loader.current = load;
  const snapshot = useSyncExternalStore(subscribeHome, useCallback(() => homeSnapshot(key), [key]));
  const refresh = useCallback(() => refreshHomeData(key, () => loader.current()), [key]);
  useEffect(() => {
    if (!enabled) return;
    void refresh();
    const unsubscribe = onHomeInvalidated(() => { void refresh(); });
    const onVisible = () => { if (document.visibilityState === 'visible') void refresh(); };
    window.addEventListener('focus', onVisible);
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      unsubscribe();
      window.removeEventListener('focus', onVisible);
      document.removeEventListener('visibilitychange', onVisible);
    };
  }, [enabled, refresh, refreshKey]);
  return { data: (snapshot.data as T | undefined) ?? fallback, loading: enabled && snapshot.loading, refresh };
}

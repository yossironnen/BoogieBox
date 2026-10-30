/**
 * Re-checks an open smart playlist at most once a minute while the page is visible, and when the
 * page comes back after a minute or more away. The server answers from stored tracks unless its
 * rules' inputs changed, so the check is cheap.
 */

import { useEffect, useRef } from 'react';

export const SMART_RECHECK_MS = 60_000;

export function useSmartRecheck(enabled: boolean, recheck: () => void): void {
  const recheckRef = useRef(recheck);
  recheckRef.current = recheck;
  useEffect(() => {
    if (!enabled) return undefined;
    let last = Date.now();
    const run = () => {
      // A second of slack so timer jitter doesn't skip a whole interval.
      if (document.visibilityState !== 'visible' || Date.now() - last < SMART_RECHECK_MS - 1_000) return;
      last = Date.now();
      recheckRef.current();
    };
    const id = window.setInterval(run, SMART_RECHECK_MS);
    document.addEventListener('visibilitychange', run);
    return () => {
      window.clearInterval(id);
      document.removeEventListener('visibilitychange', run);
    };
  }, [enabled]);
}

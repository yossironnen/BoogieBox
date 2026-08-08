/**
 * Defines Use Scan Activity Refresh behavior for BoogieBox.
 *
 * Background scans keep adding tracks, albums, and artists long after a view has
 * mounted. This hook polls the active scan jobs and invokes `onRefresh` while a
 * scan is running, plus once more after the last job finishes, so views stay
 * current without the user reloading the page.
 */

import { useEffect, useRef } from 'react';
import { api } from '../api';

export const SCAN_ACTIVITY_POLL_MS = 5000;

/** Use Scan Activity Refresh is part of this module's public API. */
export function useScanActivityRefresh(
  onRefresh: () => void | Promise<void>,
  intervalMs: number = SCAN_ACTIVITY_POLL_MS,
): void {
  const onRefreshRef = useRef(onRefresh);
  onRefreshRef.current = onRefresh;

  useEffect(() => {
    let cancelled = false;
    let pollBusy = false;
    let scanWasActive = false;

    const pollScanActivity = async () => {
      if (pollBusy) return;
      pollBusy = true;
      try {
        const scanActive = (await api.scanJobs.active()).length > 0;
        if (cancelled) return;
        if (scanActive || scanWasActive) await onRefreshRef.current();
        scanWasActive = scanActive;
      } catch {
        // Ignore transient polling failures.
      } finally {
        pollBusy = false;
      }
    };

    const pollId = setInterval(pollScanActivity, intervalMs);
    return () => { cancelled = true; clearInterval(pollId); };
  }, [intervalMs]);
}

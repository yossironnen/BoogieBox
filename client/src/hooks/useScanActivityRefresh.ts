/**
 * Defines Use Scan Activity Refresh behavior for BoogieBox.
 *
 * Background scans keep adding tracks, albums, and artists long after a view has
 * mounted. This hook polls the active scan jobs and invokes `onRefresh` while a
 * scan is running, plus once more after the last job finishes, so views stay
 * current without the user reloading the page.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../api';
import type { ScanJob } from '../types';

export const SCAN_ACTIVITY_POLL_MS = 5000;

/** Scan Activity State is part of this module's public API. */
export interface ScanActivityState {
  activeJobs: ScanJob[];
  refresh: () => Promise<void>;
}

/** Describes why a scan-driven view refresh is running. */
export interface ScanRefreshContext {
  scanActive: boolean;
  scanFinished: boolean;
}

/** Use Scan Activity Refresh is part of this module's public API. */
export function useScanActivityRefresh(
  onRefresh: (context: ScanRefreshContext) => void | Promise<void>,
  intervalMs: number = SCAN_ACTIVITY_POLL_MS,
): ScanActivityState {
  const onRefreshRef = useRef(onRefresh);
  const pollRef = useRef<(() => Promise<void>) | null>(null);
  onRefreshRef.current = onRefresh;
  const [activeJobs, setActiveJobs] = useState<ScanJob[]>([]);
  const refresh = useCallback(async () => {
    await pollRef.current?.();
  }, []);

  useEffect(() => {
    let cancelled = false;
    let pollBusy = false;
    let scanWasActive = false;

    const pollScanActivity = async () => {
      if (pollBusy) return;
      pollBusy = true;
      try {
        const jobs = await api.scanJobs.active();
        const scanActive = jobs.length > 0;
        const scanFinished = !scanActive && scanWasActive;
        if (cancelled) return;
        setActiveJobs(jobs);
        if (scanActive || scanFinished) {
          await onRefreshRef.current({ scanActive, scanFinished });
        }
        scanWasActive = scanActive;
      } catch {
        // Ignore transient polling failures.
      } finally {
        pollBusy = false;
      }
    };

    pollRef.current = pollScanActivity;
    void pollScanActivity();
    const pollId = setInterval(pollScanActivity, intervalMs);
    return () => {
      cancelled = true;
      if (pollRef.current === pollScanActivity) pollRef.current = null;
      clearInterval(pollId);
    };
  }, [intervalMs]);

  return { activeJobs, refresh };
}

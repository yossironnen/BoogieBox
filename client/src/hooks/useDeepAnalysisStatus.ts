/** Polls the authenticated BoogieMix deep-analysis status without clearing stale data on errors. */

import { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../api';
import type { BoogieMixDeepAnalysisStatus } from '../types';

export const DEEP_ANALYSIS_STATUS_POLL_MS = 60000;

/** Deep Analysis Poll State is part of this module's public API. */
export interface DeepAnalysisPollState {
  status: BoogieMixDeepAnalysisStatus | null;
  refresh: () => Promise<void>;
}

export function useDeepAnalysisStatus(
  enabled: boolean,
  intervalMs: number = DEEP_ANALYSIS_STATUS_POLL_MS,
): DeepAnalysisPollState {
  const [status, setStatus] = useState<BoogieMixDeepAnalysisStatus | null>(null);
  const pollRef = useRef<(() => Promise<void>) | null>(null);
  const refresh = useCallback(async () => {
    await pollRef.current?.();
  }, []);

  useEffect(() => {
    if (!enabled) {
      setStatus(null);
      return undefined;
    }

    let cancelled = false;
    let pollBusy = false;
    const poll = async () => {
      if (pollBusy) return;
      pollBusy = true;
      try {
        const nextStatus = await api.boogiemix.deepAnalysisStatus();
        if (!cancelled) setStatus(nextStatus);
      } catch {
        // Keep the last successful status through transient failures.
      } finally {
        pollBusy = false;
      }
    };

    pollRef.current = poll;
    void poll();
    const pollId = window.setInterval(poll, intervalMs);
    return () => {
      cancelled = true;
      if (pollRef.current === poll) pollRef.current = null;
      window.clearInterval(pollId);
    };
  }, [enabled, intervalMs]);

  return { status, refresh };
}

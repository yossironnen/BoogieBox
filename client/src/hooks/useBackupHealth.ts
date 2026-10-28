/**
 * Admin-only backup health for the sidebar warning, refreshed every few minutes.
 */

import { useEffect, useState } from 'react';
import { api } from '../api';
import type { BackupHealth } from '../types';

const REFRESH_MS = 5 * 60_000;

export function useBackupHealth(enabled: boolean): BackupHealth | null {
  const [health, setHealth] = useState<BackupHealth | null>(null);
  useEffect(() => {
    if (!enabled) {
      setHealth(null);
      return undefined;
    }
    let cancelled = false;
    const load = () => {
      api.backups.health()
        .then((next) => { if (!cancelled) setHealth(next); })
        .catch(() => {});
    };
    load();
    const id = window.setInterval(load, REFRESH_MS);
    return () => { cancelled = true; window.clearInterval(id); };
  }, [enabled]);
  return health;
}

/**
 * Whole-app "a backup is being restored" state. The admin who starts a restore drives it directly; every other
 * browser learns from a 503 `restoring` answer and waits for the server to come back.
 */

import { useSyncExternalStore } from 'react';

export type RestoreOverlay =
  | { phase: 'restoring'; startedHere: boolean }
  /** `backupDate` is the restored backup's creation time (RFC 3339). */
  | { phase: 'done'; backupDate: string | null };

let current: RestoreOverlay | null = null;
const listeners = new Set<() => void>();

function emit() {
  listeners.forEach((listener) => listener());
}

export function getRestoreOverlay(): RestoreOverlay | null {
  return current;
}

export function subscribeRestoreOverlay(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The admin starts a restore from this browser. */
export function beginRestoreHere(): void {
  current = { phase: 'restoring', startedHere: true };
  emit();
}

/** The restore this browser started finished. */
export function finishRestoreHere(backupDate: string | null): void {
  current = { phase: 'done', backupDate };
  emit();
}

/** The restore this browser started did not happen (the server says why). */
export function cancelRestoreHere(): void {
  current = null;
  emit();
}

/** Another browser's restore is running (an API call answered 503 `restoring`). */
export function notifyServerRestoring(): void {
  if (current) return;
  current = { phase: 'restoring', startedHere: false };
  emit();
}

/** Test helper. */
export function resetRestoreOverlay(): void {
  current = null;
  emit();
}

export function useRestoreOverlay(): RestoreOverlay | null {
  return useSyncExternalStore(subscribeRestoreOverlay, getRestoreOverlay, getRestoreOverlay);
}

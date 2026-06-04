/**
 * Defines Index behavior for BoogieBox.
 */

import { browserPlatform } from './browserPlatform';
import { desktopPlatform } from './desktopPlatform';
import type { Platform } from './types';

export type { DesktopConfig, Platform, ServerProbeResult } from './types';

/** Whether the app is running inside the Tauri desktop shell. */
export function isTauriDesktop(): boolean {
  return typeof window !== 'undefined' && !!window.__TAURI__;
}

/**
 * The active platform implementation.  Import this singleton instead of
 * branching on `isTauriDesktop()` in feature components.
 */
export const platform: Platform = isTauriDesktop() ? desktopPlatform : browserPlatform;

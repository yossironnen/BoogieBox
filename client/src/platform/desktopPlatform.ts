/**
 * Defines Desktop Platform behavior for BoogieBox.
 */

import type { DesktopConfig, Platform, ServerDiscoveryResult, ServerProbeResult } from './types';

// Tauri injects __TAURI__ into the webview. We use the core invoke function
// and the event system directly so this module has no hard build-time dependency
// on @tauri-apps/api (which only exists inside the desktop package).
declare global {
  interface Window {
    __TAURI__?: {
      core: {
        invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T>;
      };
      event: {
        listen<T>(event: string, handler: (evt: { payload: T }) => void): Promise<() => void>;
      };
    };
  }
}

function invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!window.__TAURI__) throw new Error('Tauri not available');
  return window.__TAURI__.core.invoke<T>(cmd, args);
}

export const desktopPlatform: Platform = {
  isDesktop: true,

  async getConfig(): Promise<DesktopConfig | null> {
    return invoke<DesktopConfig>('get_config');
  },

  async probeServer(url?: string): Promise<ServerProbeResult | null> {
    return invoke<ServerProbeResult>('probe_server', { url });
  },

  async discoverServers(): Promise<ServerDiscoveryResult | null> {
    return invoke<ServerDiscoveryResult>('discover_servers');
  },

  async selectFolder(initialDir?: string): Promise<string | null> {
    return invoke<string | null>('select_folder', { initialDir });
  },
};

/**
 * Defines Browser Platform behavior for BoogieBox.
 */

import type { DesktopConfig, Platform, ServerDiscoveryResult, ServerProbeResult } from './types';

export const browserPlatform: Platform = {
  isDesktop: false,

  async getConfig(): Promise<DesktopConfig | null> {
    return null;
  },

  async probeServer(_url?: string): Promise<ServerProbeResult | null> {
    return null;
  },

  async discoverServers(): Promise<ServerDiscoveryResult | null> {
    return null;
  },

  async selectFolder(_initialDir?: string): Promise<string | null> {
    const res = await fetch('/api/system/select-folder', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      credentials: 'include',
      body: JSON.stringify({ initialDir: _initialDir }),
    });
    const payload = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(payload.error || 'Folder picker failed');
    return typeof payload.folder === 'string' && payload.folder.trim() ? payload.folder : null;
  },
};

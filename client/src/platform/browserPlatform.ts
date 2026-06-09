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
    // Try the authenticated admin endpoint first (post-setup); fall back to the
    // setup-only loopback endpoint during first-run setup.
    for (const url of ['/api/admin/browse-folder', '/api/system/select-folder']) {
      const res = await fetch(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
        body: JSON.stringify({ initialDir: _initialDir }),
      });
      if (res.status === 400 && url === '/api/system/select-folder') break;
      const payload = await res.json().catch(() => ({}));
      if (!res.ok) {
        if (url === '/api/admin/browse-folder' && (res.status === 401 || res.status === 403)) continue;
        throw new Error(payload.error || 'Folder picker failed');
      }
      return typeof payload.folder === 'string' && payload.folder.trim() ? payload.folder : null;
    }
    return null;
  },
};

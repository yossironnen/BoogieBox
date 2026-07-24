import { afterEach, describe, expect, it, vi } from 'vitest';
import { desktopPlatform } from './desktopPlatform';

describe('desktopPlatform', () => {
  afterEach(() => {
    delete window.__TAURI__;
  });

  it('reports a clear error outside Tauri', async () => {
    await expect(desktopPlatform.getConfig()).rejects.toThrow('Tauri not available');
  });

  it('maps every platform operation to the expected Tauri command', async () => {
    const invoke = vi.fn()
      .mockResolvedValueOnce({ serverUrl: 'http://server' })
      .mockResolvedValueOnce({ ok: true })
      .mockResolvedValueOnce({ servers: [] })
      .mockResolvedValueOnce('C:\\Music');
    window.__TAURI__ = {
      core: { invoke },
      event: { listen: vi.fn() },
    };

    await expect(desktopPlatform.getConfig()).resolves.toEqual({ serverUrl: 'http://server' });
    await expect(desktopPlatform.probeServer()).resolves.toEqual({ ok: true });
    await expect(desktopPlatform.discoverServers()).resolves.toEqual({ servers: [] });
    await expect(desktopPlatform.selectFolder('C:\\')).resolves.toBe('C:\\Music');
    expect(invoke.mock.calls).toEqual([
      ['get_config', undefined],
      ['probe_server', { url: undefined }],
      ['discover_servers', undefined],
      ['select_folder', { initialDir: 'C:\\' }],
    ]);
    expect(desktopPlatform.isDesktop).toBe(true);
  });
});

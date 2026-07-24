import { beforeEach, describe, expect, it, vi } from 'vitest';
import { browserPlatform } from './browserPlatform';

function response(ok: boolean, status: number, body: unknown) {
  return { ok, status, json: vi.fn().mockResolvedValue(body) } as unknown as Response;
}

describe('browserPlatform', () => {
  beforeEach(() => vi.restoreAllMocks());

  it('reports browser-only capabilities', async () => {
    expect(browserPlatform.isDesktop).toBe(false);
    await expect(browserPlatform.getConfig()).resolves.toBeNull();
    await expect(browserPlatform.probeServer('http://server')).resolves.toBeNull();
    await expect(browserPlatform.discoverServers()).resolves.toBeNull();
  });

  it('selects through the authenticated endpoint and normalizes empty choices', async () => {
    vi.stubGlobal('fetch', vi.fn()
      .mockResolvedValueOnce(response(true, 200, { folder: 'D:\\Music' }))
      .mockResolvedValueOnce(response(true, 200, { folder: '   ' })));
    await expect(browserPlatform.selectFolder('D:\\')).resolves.toBe('D:\\Music');
    await expect(browserPlatform.selectFolder()).resolves.toBeNull();
  });

  it('falls back to setup selection and handles cancellation or errors', async () => {
    vi.stubGlobal('fetch', vi.fn()
      .mockResolvedValueOnce(response(false, 401, { error: 'auth required' }))
      .mockResolvedValueOnce(response(true, 200, { folder: 'D:\\Data' })));
    await expect(browserPlatform.selectFolder('D:\\')).resolves.toBe('D:\\Data');

    vi.mocked(fetch)
      .mockResolvedValueOnce(response(false, 401, {}))
      .mockResolvedValueOnce(response(false, 400, {}));
    await expect(browserPlatform.selectFolder()).resolves.toBeNull();

    vi.mocked(fetch)
      .mockResolvedValueOnce(response(false, 401, {}))
      .mockResolvedValueOnce(response(false, 500, { error: 'picker crashed' }));
    await expect(browserPlatform.selectFolder()).rejects.toThrow('picker crashed');
  });
});

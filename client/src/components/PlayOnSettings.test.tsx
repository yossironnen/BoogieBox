import React from 'react';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import type { RendererInfo } from '../remote/remoteTypes';
import PlayOnSettings, { rendererDetail } from './PlayOnSettings';

const wiim: RendererInfo = {
  udn: 'uuid:wiim', name: 'WiiM Ultra', friendlyName: 'WiiM Ultra', alias: null, manufacturer: 'Linkplay',
  model: 'WiiM Ultra Receiver', vendor: 'wiim', online: true, hidden: false, formatPolicy: 'auto', hasIcon: false,
  capabilities: { setNext: true, seek: true, volume: true }, lastSeenAt: '2026-10-12 10:00:00',
  session: { status: 'playing', ownerUserId: 'u2', ownerName: 'Dana', nowPlaying: 'Fragments', heldByOther: true },
};
const tv: RendererInfo = {
  ...wiim, udn: 'uuid:tv', name: 'Living room TV', friendlyName: 'BRAVIA', alias: 'Living room TV', manufacturer: 'Sony',
  model: 'XBR-75X900E', online: false, formatPolicy: 'mp3', session: null,
};

describe('PlayOnSettings', () => {
  beforeEach(() => {
    vi.spyOn(api.settings, 'get').mockResolvedValue({ remotePlaybackEnabled: 'true', remoteReleaseStopsRenderer: 'false', remoteInterfaceIp: '' });
    vi.spyOn(api.settings, 'update').mockResolvedValue({ ok: true });
    vi.spyOn(api.remote, 'renderers').mockResolvedValue({ running: true, renderers: [wiim, tv] });
    vi.spyOn(api.remote, 'diagnostics').mockResolvedValue({
      enabled: true, running: true, detectedInterfaces: ['10.0.0.227', '172.26.96.1'], searchInterfaces: ['10.0.0.227', '172.26.96.1'],
      lastSearchMs: Date.UTC(2026, 9, 12, 14, 5), lastError: null, httpPort: 3001, renderers: 2, online: 1, sessions: 1,
    });
    vi.spyOn(api.remote, 'updateRenderer').mockResolvedValue({ ok: true });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('lists speakers with status, details and the interface choices', async () => {
    render(<PlayOnSettings />);
    const list = await screen.findByRole('list', { name: 'Speakers' });
    const rows = within(list).getAllByRole('listitem');
    expect(rows).toHaveLength(2);
    expect(within(rows[0]).getByText('Online')).toBeInTheDocument();
    expect(within(rows[0]).getByText('Linkplay · WiiM Ultra Receiver · gapless, seek')).toBeInTheDocument();
    expect(within(rows[1]).getByText('Offline')).toBeInTheDocument();
    expect(within(rows[1]).getByLabelText('Format for Living room TV')).toHaveValue('mp3');
    expect(screen.getByRole('switch', { name: 'Stop the speaker when switching back to a browser' })).toHaveAttribute('aria-checked', 'false');
    await waitFor(() => expect(screen.getByRole('option', { name: 'Automatic (10.0.0.227, 172.26.96.1)' })).toBeInTheDocument());
    const diagnostics = screen.getByLabelText('Play On diagnostics');
    expect(diagnostics).toHaveTextContent('Searching on 10.0.0.227 and 172.26.96.1 · last search');
    expect(diagnostics).toHaveTextContent('Active sessions: 1 (WiiM Ultra, Dana)');
    expect(diagnostics).toHaveTextContent('No errors');
  });

  it('saves switches, renames, format and visibility', async () => {
    render(<PlayOnSettings />);
    await screen.findByRole('list', { name: 'Speakers' });

    fireEvent.click(screen.getByRole('switch', { name: 'Enable Play On' }));
    await waitFor(() => expect(api.settings.update).toHaveBeenCalledWith({ remotePlaybackEnabled: 'false' }));

    const name = screen.getByLabelText('Display name for WiiM Ultra');
    fireEvent.change(name, { target: { value: ' Den ' } });
    fireEvent.blur(name);
    await waitFor(() => expect(api.remote.updateRenderer).toHaveBeenCalledWith('uuid:wiim', { alias: 'Den' }));

    fireEvent.change(screen.getByLabelText('Display name for BRAVIA'), { target: { value: '' } });
    fireEvent.blur(screen.getByLabelText('Display name for BRAVIA'));
    await waitFor(() => expect(api.remote.updateRenderer).toHaveBeenCalledWith('uuid:tv', { alias: null }));

    fireEvent.change(screen.getByLabelText('Format for WiiM Ultra'), { target: { value: 'lossless' } });
    await waitFor(() => expect(api.remote.updateRenderer).toHaveBeenCalledWith('uuid:wiim', { formatPolicy: 'lossless' }));

    fireEvent.click(screen.getByRole('switch', { name: 'Hide WiiM Ultra' }));
    await waitFor(() => expect(api.remote.updateRenderer).toHaveBeenCalledWith('uuid:wiim', { hidden: true }));
    expect(await screen.findAllByText('Saved')).not.toHaveLength(0);
  });

  it('scans and reports how many speakers were found', async () => {
    const refresh = vi.spyOn(api.remote, 'refresh').mockResolvedValue({ ok: true });
    render(<PlayOnSettings />);
    await screen.findByRole('list', { name: 'Speakers' });
    vi.useFakeTimers();
    fireEvent.click(screen.getByRole('button', { name: 'Scan now' }));
    expect(refresh).toHaveBeenCalled();
    await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
    vi.useRealTimers();
    expect(screen.getByText('Found 2 speakers')).toBeInTheDocument();
  });

  it('describes offline speakers by when they were last seen', () => {
    expect(rendererDetail(tv)).toMatch(/^Sony · XBR-75X900E · last seen /);
    expect(rendererDetail({ ...wiim, capabilities: { setNext: false, seek: false, volume: false } })).toBe('Linkplay · WiiM Ultra Receiver · no volume control');
  });
});

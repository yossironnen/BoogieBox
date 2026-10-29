import React from 'react';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PlaylistImportPreview } from '../types';
import { PlaylistImportDialog, PlaylistImportToast } from './PlaylistImport';
import MobilePlaylistImportSheet from '../mobile/components/MobilePlaylistImportSheet';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    playlists: {
      importPreview: vi.fn(),
      importCommit: vi.fn(),
    },
  },
}));

vi.mock('../api', () => ({ api: apiMock }));

const preview: PlaylistImportPreview = {
  entries: [
    { line: 2, raw: 'D:\\Music\\a.mp3', status: 'matched', trackId: 'a', label: 'a' },
    {
      line: 4, raw: 'Teardrop.mp3', status: 'ambiguous', label: 'Massive Attack – Teardrop', suggestedTrackId: 't1',
      candidates: [
        { trackId: 't1', title: 'Teardrop', artist: 'Massive Attack', album: 'Mezzanine', year: 1998, duration: 329 },
        { trackId: 't2', title: 'Teardrop', artist: 'Massive Attack', album: 'Collected', year: 2006, duration: 331 },
      ],
    },
    { line: 6, raw: 'D:\\Rips\\gone.flac', status: 'unmatched', label: 'gone' },
    { line: 7, raw: 'https://radio.example/live', status: 'remote', label: 'live' },
    { line: 8, raw: 'D:\\Music\\a.mp3', status: 'duplicate', trackId: 'a', label: 'a' },
  ],
  totals: { matched: 1, ambiguous: 1, unmatched: 1, remote: 1, duplicate: 1 },
  suggestedRemaps: [{ from: 'D:\\Rips', to: '/mnt/rips', gain: 1 }],
  appliedRemaps: [],
};

const afterSwap: PlaylistImportPreview = {
  ...preview,
  entries: preview.entries.map((e) => (e.line === 6 ? { ...e, status: 'matched', trackId: 'g' } : e)),
  totals: { matched: 2, ambiguous: 1, unmatched: 0, remote: 1, duplicate: 1 },
  suggestedRemaps: [],
  appliedRemaps: [{ from: 'D:\\Rips', to: '/mnt/rips', gain: 1 }],
};

const file = (text = '#EXTM3U\n', name = 'Road Trip 2026.m3u8') => new File([text], name);
const tile = (key: string) => document.querySelector(`[data-import-tile="${key}"]`) as HTMLElement;

describe('PlaylistImportDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMock.playlists.importPreview.mockResolvedValue(preview);
  });

  it('previews the file with tiles, choices and the not-found list, then creates in order', async () => {
    apiMock.playlists.importCommit.mockResolvedValue({ playlist: { id: 'p1', name: 'Road Trip 2026' }, added: 2 });
    const onImported = vi.fn();
    render(<PlaylistImportDialog file={file()} onClose={vi.fn()} onImported={onImported} />);

    const dialog = await screen.findByRole('dialog', { name: 'Import playlist' });
    await waitFor(() => expect(tile('matched')).toHaveTextContent('1Found'));
    expect(apiMock.playlists.importPreview).toHaveBeenCalledWith(btoa('#EXTM3U\n'), []);
    expect(within(dialog).getByText('Road Trip 2026.m3u8')).toBeInTheDocument();
    expect(within(dialog).getByText('· 5 lines')).toBeInTheDocument();
    expect(within(dialog).getByLabelText('Playlist name')).toHaveValue('Road Trip 2026');
    expect(tile('ambiguous')).toHaveTextContent('1Choose one');
    expect(tile('unmatched')).toHaveTextContent('1Not found');
    expect(tile('remote')).toHaveTextContent('1Web links');
    expect(tile('duplicate')).toHaveTextContent('1Repeats');
    expect(within(dialog).getByText('Some of this music is in a different folder here')).toBeInTheDocument();
    expect(within(dialog).getByRole('radio', { name: /Teardrop · Mezzanine \(1998\) · 5:29/ })).toBeChecked();
    expect(within(dialog).getByText('Line 6')).toBeInTheDocument();
    expect(within(dialog).getByText("Web links aren't imported. BoogieBox plays only music in your libraries.")).toBeInTheDocument();
    expect(within(dialog).getByText('A playlist holds each track once, so repeated lines are added the first time only.')).toBeInTheDocument();
    expect(within(dialog).getByRole('status')).toHaveTextContent("2 tracks will be added, in the file's order");

    fireEvent.click(within(dialog).getByRole('radio', { name: /Collected/ }));
    fireEvent.change(within(dialog).getByLabelText('Playlist name'), { target: { value: '  Drive  ' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Create playlist' }));
    await waitFor(() => expect(onImported).toHaveBeenCalledWith({ playlist: { id: 'p1', name: 'Road Trip 2026' }, added: 2, leftOut: 3 }));
    expect(apiMock.playlists.importCommit).toHaveBeenCalledWith('Drive', ['a', 't2']);
  });

  it('skipping the only choice leaves it out; a folder swap re-previews and can be undone', async () => {
    render(<PlaylistImportDialog file={file()} onClose={vi.fn()} onImported={vi.fn()} />);
    const dialog = await screen.findByRole('dialog');
    await waitFor(() => expect(tile('matched')).toBeInTheDocument());
    fireEvent.click(within(dialog).getByRole('radio', { name: 'Skip this line' }));
    expect(within(dialog).getByRole('status')).toHaveTextContent('1 track will be added');

    apiMock.playlists.importPreview.mockResolvedValueOnce(afterSwap);
    fireEvent.click(within(dialog).getByRole('button', { name: 'Use /mnt/rips' }));
    await waitFor(() => expect(within(dialog).getByRole('button', { name: 'Stop using /mnt/rips' })).toBeInTheDocument());
    expect(apiMock.playlists.importPreview).toHaveBeenLastCalledWith(btoa('#EXTM3U\n'), [{ from: 'D:\\Rips', to: '/mnt/rips' }]);
    expect(within(dialog).getByRole('radio', { name: 'Skip this line' })).toBeChecked();
    expect(within(dialog).getByText('+1 found')).toBeInTheDocument();

    apiMock.playlists.importPreview.mockResolvedValueOnce(preview);
    fireEvent.click(within(dialog).getByRole('button', { name: 'Stop using /mnt/rips' }));
    await waitFor(() => expect(apiMock.playlists.importPreview).toHaveBeenLastCalledWith(btoa('#EXTM3U\n'), []));
  });

  it('shows the name-taken error under the name and other failures as an alert', async () => {
    apiMock.playlists.importCommit.mockRejectedValueOnce(new Error('A playlist with this name already exists'));
    render(<PlaylistImportDialog file={file()} onClose={vi.fn()} onImported={vi.fn()} />);
    const dialog = await screen.findByRole('dialog');
    await waitFor(() => expect(tile('matched')).toBeInTheDocument());
    fireEvent.click(within(dialog).getByRole('button', { name: 'Create playlist' }));
    expect(await within(dialog).findByText('A playlist with this name already exists')).toBeInTheDocument();
    expect(within(dialog).getByLabelText('Playlist name')).toHaveAttribute('aria-invalid', 'true');

    apiMock.playlists.importCommit.mockRejectedValueOnce(new Error('Internal server error'));
    fireEvent.change(within(dialog).getByLabelText('Playlist name'), { target: { value: 'Other' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Create playlist' }));
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('Internal server error');
  });

  it('shows the file problems with the approved copy and no Create button', async () => {
    const tooBig = new File([new Uint8Array(2 * 1024 * 1024 + 1)], 'huge.m3u');
    const { unmount } = render(<PlaylistImportDialog file={tooBig} onClose={vi.fn()} onImported={vi.fn()} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('This file is too bigPlaylist files up to 2 MB can be imported.');
    expect(apiMock.playlists.importPreview).not.toHaveBeenCalled();
    expect(screen.queryByRole('button', { name: 'Create playlist' })).not.toBeInTheDocument();
    unmount();

    for (const [code, text] of [
      ['too_long', 'This playlist is too longUp to 10,000 tracks can be imported at once. Split the file and import each part.'],
      ['empty', 'No tracks in this fileChoose an .m3u or .m3u8 playlist file.'],
    ]) {
      apiMock.playlists.importPreview.mockRejectedValueOnce(Object.assign(new Error('x'), { code }));
      const view = render(<PlaylistImportDialog file={file()} onClose={vi.fn()} onImported={vi.fn()} />);
      expect(await screen.findByRole('alert')).toHaveTextContent(text);
      view.unmount();
    }
  });

  it('when nothing matched it explains why and disables Create', async () => {
    apiMock.playlists.importPreview.mockResolvedValueOnce({
      entries: [{ line: 1, raw: 'x.mp3', status: 'unmatched', label: 'x' }],
      totals: { matched: 0, ambiguous: 0, unmatched: 1, remote: 0, duplicate: 0 },
      suggestedRemaps: [],
      appliedRemaps: [],
    });
    render(<PlaylistImportDialog file={file()} onClose={vi.fn()} onImported={vi.fn()} />);
    expect(await screen.findByText('None of these tracks are in your libraries')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Create playlist' })).toBeDisabled();
  });

  it('downloads the not-found list and closes on Escape', async () => {
    const created = vi.fn(() => 'blob:x');
    const revoked = vi.fn();
    Object.assign(URL, { createObjectURL: created, revokeObjectURL: revoked });
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {});
    const onClose = vi.fn();
    render(<PlaylistImportDialog file={file()} onClose={onClose} onImported={vi.fn()} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Download the list' }));
    expect(created).toHaveBeenCalled();
    expect(click).toHaveBeenCalled();
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).toHaveBeenCalled();
    click.mockRestore();
  });
});

describe('MobilePlaylistImportSheet', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMock.playlists.importPreview.mockResolvedValue(preview);
  });

  it('shows the same preview in a sheet and creates the playlist', async () => {
    apiMock.playlists.importCommit.mockResolvedValue({ playlist: { id: 'p1', name: 'Road Trip 2026' }, added: 2 });
    const onImported = vi.fn();
    render(<MobilePlaylistImportSheet file={file()} onClose={vi.fn()} onImported={onImported} />);
    expect(await screen.findByRole('heading', { name: 'Import playlist' })).toBeInTheDocument();
    await waitFor(() => expect(tile('matched')).toBeInTheDocument());
    expect(screen.getByText('Not found · 1')).toBeInTheDocument();
    expect(screen.getByText('Web links and repeats are left out too.')).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: 'Mezzanine (1998) · 5:29' })).toBeChecked();
    fireEvent.click(screen.getByRole('button', { name: 'Create playlist' }));
    await waitFor(() => expect(onImported).toHaveBeenCalled());
  });
});

describe('PlaylistImportToast', () => {
  afterEach(() => vi.useRealTimers());

  it('reports what was added and dismisses itself', () => {
    vi.useFakeTimers();
    const onDismiss = vi.fn();
    render(<PlaylistImportToast added={197} leftOut={33} onDismiss={onDismiss} />);
    expect(screen.getByRole('status')).toHaveTextContent('Playlist imported197 tracks added. 33 lines were left out.');
    act(() => { vi.advanceTimersByTime(8000); });
    expect(onDismiss).toHaveBeenCalled();
  });
});

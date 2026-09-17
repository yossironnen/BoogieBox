/**
 * Tests Mobile Mixes View behavior for BoogieBox regressions.
 */

import React from 'react';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { describe, expect, it, vi, beforeEach } from 'vitest';
import type { BoogieMixOutput, ClientEntityId } from '../../types';
import MobileMixesView from './MobileMixesView';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    boogiemix: {
      listAllOutputs: vi.fn(),
      renameOutput: vi.fn(),
      deleteOutput: vi.fn(),
      playUrl: vi.fn((outputId: string) => `/api/boogiemix/outputs/${outputId}/play`),
    },
    albumArtUrl: vi.fn((albumId: ClientEntityId, size: number) => `/api/albums/${albumId}/art?size=${size}`),
  },
}));

vi.mock('../../api', () => ({ api: apiMock }));

const output: BoogieMixOutput = {
  id: 'mix-1',
  job_id: 'job-1',
  playlist_id: '7',
  file_name: 'evening.mp3',
  name: 'Evening Wind Down',
  playlist_name: 'Chill Evenings',
  cover_album_ids: '["31","32"]',
  duration_sec: 1830,
  file_size_bytes: 5000,
  format: 'mp3',
  created_at: '2026-03-10T00:00:00.000Z',
};

describe('MobileMixesView', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMock.boogiemix.listAllOutputs.mockResolvedValue([output]);
    apiMock.boogiemix.renameOutput.mockResolvedValue({ ok: true });
    apiMock.boogiemix.deleteOutput.mockResolvedValue({ ok: true });
  });

  it('lists mixes with cover art and a formatted duration', async () => {
    render(<MobileMixesView onPlayTrack={() => {}} onOpenPlaylist={() => {}} />);

    expect(await screen.findByText('Evening Wind Down')).toBeInTheDocument();
    expect(screen.getByText('From Chill Evenings')).toBeInTheDocument();
    expect(screen.getByText('30m')).toBeInTheDocument();
    expect(apiMock.albumArtUrl).toHaveBeenCalledWith('31', 300);
  });

  it('falls back to a placeholder and a deleted-source note when data is missing', async () => {
    apiMock.boogiemix.listAllOutputs.mockResolvedValue([
      { ...output, playlist_id: null, playlist_name: null, cover_album_ids: 'not-json', duration_sec: null },
    ]);
    render(<MobileMixesView onPlayTrack={() => {}} onOpenPlaylist={() => {}} />);

    expect(await screen.findByText('Source playlist deleted')).toBeInTheDocument();
    expect(screen.getByText('--')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'More actions for Evening Wind Down' }));
    expect(screen.queryByRole('button', { name: /Open source playlist/i })).not.toBeInTheDocument();
  });

  it('plays a mix as a synthesized track', async () => {
    const onPlayTrack = vi.fn();
    render(<MobileMixesView onPlayTrack={onPlayTrack} onOpenPlaylist={() => {}} />);

    fireEvent.click(await screen.findByRole('button', { name: 'Play Evening Wind Down' }));
    expect(onPlayTrack).toHaveBeenCalledWith(
      expect.objectContaining({ id: 'boogiemix:mix-1', title: 'Evening Wind Down' }),
      expect.any(Array),
    );
  });

  it('opens the source playlist from the actions sheet', async () => {
    const onOpenPlaylist = vi.fn();
    render(<MobileMixesView onPlayTrack={() => {}} onOpenPlaylist={onOpenPlaylist} />);

    fireEvent.click(await screen.findByRole('button', { name: 'More actions for Evening Wind Down' }));
    fireEvent.click(screen.getByRole('button', { name: /Open source playlist/i }));
    expect(onOpenPlaylist).toHaveBeenCalledWith('7');
  });

  it('renames a mix, rolling back on failure', async () => {
    render(<MobileMixesView onPlayTrack={() => {}} onOpenPlaylist={() => {}} />);
    fireEvent.click(await screen.findByRole('button', { name: 'More actions for Evening Wind Down' }));
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    fireEvent.change(screen.getByDisplayValue('Evening Wind Down'), { target: { value: 'Renamed' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('Renamed')).toBeInTheDocument();
    expect(apiMock.boogiemix.renameOutput).toHaveBeenCalledWith('mix-1', 'Renamed');

    apiMock.boogiemix.renameOutput.mockRejectedValueOnce(new Error('nope'));
    apiMock.boogiemix.listAllOutputs.mockResolvedValueOnce([output]);
    fireEvent.click(screen.getByRole('button', { name: 'More actions for Renamed' }));
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    fireEvent.change(screen.getByDisplayValue('Renamed'), { target: { value: 'Bad Name' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('Evening Wind Down')).toBeInTheDocument();
  });

  it('deletes a mix after confirmation', async () => {
    render(<MobileMixesView onPlayTrack={() => {}} onOpenPlaylist={() => {}} />);
    fireEvent.click(await screen.findByRole('button', { name: 'More actions for Evening Wind Down' }));
    fireEvent.click(screen.getByRole('button', { name: /Delete mix/i }));
    const dialog = await screen.findByRole('alertdialog', { name: 'Delete this mix?' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Delete mix' }));
    await waitFor(() => expect(apiMock.boogiemix.deleteOutput).toHaveBeenCalledWith('mix-1'));
    expect(screen.queryByText('Evening Wind Down')).not.toBeInTheDocument();
  });

  it('shows an error with retry when loading fails', async () => {
    apiMock.boogiemix.listAllOutputs.mockRejectedValueOnce(new Error('network down'));
    render(<MobileMixesView onPlayTrack={() => {}} onOpenPlaylist={() => {}} />);
    expect(await screen.findByText('network down')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(await screen.findByText('Evening Wind Down')).toBeInTheDocument();
  });

  it('shows an empty state when there are no mixes', async () => {
    apiMock.boogiemix.listAllOutputs.mockResolvedValue([]);
    render(<MobileMixesView onPlayTrack={() => {}} onOpenPlaylist={() => {}} />);
    expect(await screen.findByText('No mixes yet.')).toBeInTheDocument();
  });
});

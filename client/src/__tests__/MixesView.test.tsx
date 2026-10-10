/**
 * Tests MixesView behavior for BoogieBox regressions, focused on the new
 * "View breakdown" entry point into MixStoryView
 * (wip/boogiemix-story-timeline-plan.md Phase 2).
 */

import React from 'react';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import MixesView from '../components/MixesView';
import type { BoogieMixOutput, MixTimelineResponse } from '../types';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    boogiemix: {
      listAllOutputs: vi.fn(),
      renameOutput: vi.fn(),
      deleteOutput: vi.fn(),
      outputDownloadUrl: vi.fn((id: string) => `/api/boogiemix/outputs/${id}/file`),
      playUrl: vi.fn((id: string) => `/api/boogiemix/outputs/${id}/play`),
      timeline: vi.fn(),
      storyImageUrl: vi.fn((id: string) => `/api/boogiemix/outputs/${id}/story-image`),
    },
    albumArtUrl: vi.fn((id: string, size: number) => `/api/albums/${id}/art?size=${size}`),
  },
}));

vi.mock('../api', () => ({ api: apiMock }));

function output(overrides: Partial<BoogieMixOutput> = {}): BoogieMixOutput {
  return {
    id: 'output-1',
    job_id: 'job-1',
    playlist_id: 'playlist-1',
    file_name: 'mix.mp3',
    name: 'Electronic House',
    playlist_name: 'House Sessions',
    cover_album_ids: null,
    duration_sec: 600,
    file_size_bytes: 12345,
    format: 'mp3',
    created_at: '2026-09-16T00:00:00Z',
    ...overrides,
  };
}

describe('MixesView', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMock.boogiemix.listAllOutputs.mockResolvedValue([output()]);
    apiMock.boogiemix.timeline.mockResolvedValue({
      outputId: 'output-1', available: false, tier: 'unavailable', durationSec: 0, tracks: [],
    } as MixTimelineResponse);
  });

  it('opens the Mix Story view when a card\'s "View breakdown" action is clicked', async () => {
    render(<MixesView playTrack={() => {}} onOpenPlaylist={() => {}} />);

    await waitFor(() => expect(screen.getByText('Electronic House')).toBeInTheDocument());
    fireEvent.click(screen.getByRole('button', { name: /View breakdown of Electronic House/i }));

    await waitFor(() => expect(apiMock.boogiemix.timeline).toHaveBeenCalledWith('output-1'));
    expect(screen.getByRole('button', { name: /Back to Mixes/i })).toBeInTheDocument();
  });

  it('opens the Mix Story view when the card cover art is clicked', async () => {
    render(<MixesView playTrack={() => {}} onOpenPlaylist={() => {}} />);

    await waitFor(() => expect(screen.getByText('Electronic House')).toBeInTheDocument());
    fireEvent.click(screen.getByTitle('View breakdown: Electronic House'));

    await waitFor(() => expect(apiMock.boogiemix.timeline).toHaveBeenCalledWith('output-1'));
  });

  it('returns to the Mixes list when the Mix Story back button is clicked', async () => {
    render(<MixesView playTrack={() => {}} onOpenPlaylist={() => {}} />);

    await waitFor(() => expect(screen.getByText('Electronic House')).toBeInTheDocument());
    fireEvent.click(screen.getByRole('button', { name: /View breakdown of Electronic House/i }));
    await waitFor(() => expect(screen.getByRole('button', { name: /Back to Mixes/i })).toBeInTheDocument());

    fireEvent.click(screen.getByRole('button', { name: /Back to Mixes/i }));

    expect(screen.getByPlaceholderText('Search mixes or playlists…')).toBeInTheDocument();
  });

  it('uses the shared command bar for the Mixes list and the Mix Story header', async () => {
    const { container } = render(<MixesView playTrack={() => {}} onOpenPlaylist={() => {}} />);
    await waitFor(() => expect(screen.getByText('Electronic House')).toBeInTheDocument());

    let bar = container.querySelector('[data-command-bar="mixes"]') as HTMLElement;
    expect(within(bar).getByTestId('command-bar-watermark')).toHaveTextContent('Mixes');
    expect(within(bar).getByPlaceholderText('Search mixes or playlists…')).toBeInTheDocument();
    expect(within(bar).getByLabelText('1 mix')).toHaveTextContent('1');
    expect(within(bar).getByLabelText('Sort mixes')).toBeInTheDocument();
    expect(within(bar).getByRole('button', { name: 'Switch to table view' })).toBeInTheDocument();
    expect(screen.queryByText('Manage the BoogieMix collection')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /View breakdown of Electronic House/i }));
    await waitFor(() => expect(screen.getByRole('button', { name: /Back to Mixes/i })).toBeInTheDocument());
    bar = container.querySelector('[data-command-bar="mixes"]') as HTMLElement;
    expect(within(bar).getByRole('button', { name: /Back to Mixes/i })).toBeInTheDocument();
    expect(within(bar).getByLabelText('Electronic House artwork')).toBeInTheDocument();
    expect(within(bar).getByRole('button', { name: 'Play Electronic House' })).toBeInTheDocument();
    expect(within(bar).getByRole('button', { name: 'Delete Electronic House' })).toBeInTheDocument();
  });
});

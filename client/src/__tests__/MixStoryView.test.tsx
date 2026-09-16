/**
 * Tests MixStoryView behavior for BoogieBox regressions
 * (wip/boogiemix-story-timeline-plan.md Phase 2).
 */

import React from 'react';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import MixStoryView from '../components/MixStoryView';
import type { BoogieMixOutput, MixOutputTrackRow, MixTimelineResponse } from '../types';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    boogiemix: {
      timeline: vi.fn(),
      outputDownloadUrl: vi.fn((id: string) => `/api/boogiemix/outputs/${id}/file`),
      playUrl: vi.fn((id: string) => `/api/boogiemix/outputs/${id}/play`),
      storyImageUrl: vi.fn((id: string) => `/api/boogiemix/outputs/${id}/story-image`),
    },
    albumArtUrl: vi.fn((id: string, size: number) => `/api/albums/${id}/art?size=${size}`),
  },
}));

vi.mock('../api', () => ({ api: apiMock }));

function track(overrides: Partial<MixOutputTrackRow> = {}): MixOutputTrackRow {
  return {
    stepIndex: 0,
    trackId: 't1',
    albumId: null,
    title: 'Nightdrive',
    artistName: 'Kollektiv Turmstrasse',
    albumName: '',
    trackDurationSec: 300,
    bpm: 124,
    keyEstimate: '8A',
    outputStartSec: 0,
    outputEndSec: 300,
    sourceTrimStartSec: 0,
    sourceTrimEndSec: 300,
    crossfadeInSec: 0,
    crossfadeOutSec: 8,
    transitionOutKind: 'beatmatch',
    transitionOutConfidence: null,
    transitionOutPhraseAligned: true,
    transitionOutReason: 'deep:club_blend|kind:beatmatch',
    waveformPeaksJson: null,
    energyCurveJson: null,
    sectionMarkersJson: null,
    ...overrides,
  };
}

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

describe('MixStoryView', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders the ordered track list once the timeline loads (full tier)', async () => {
    const timeline: MixTimelineResponse = {
      outputId: 'output-1',
      available: true,
      tier: 'full',
      durationSec: 600,
      tracks: [track(), track({ stepIndex: 1, trackId: 't2', title: 'Shelter', outputStartSec: 292, outputEndSec: 600, crossfadeOutSec: 0 })],
    };
    apiMock.boogiemix.timeline.mockResolvedValue(timeline);

    render(<MixStoryView output={output()} playTrack={() => {}} onBack={() => {}} onDelete={() => {}} />);

    await waitFor(() => expect(screen.getByTestId('track-row-0')).toBeInTheDocument());
    expect(within(screen.getByTestId('track-row-0')).getByText('Nightdrive')).toBeInTheDocument();
    expect(within(screen.getByTestId('track-row-1')).getByText('Shelter')).toBeInTheDocument();
    expect(screen.getByText('Electronic House')).toBeInTheDocument();
    // No legacy-tier banner for a full snapshot.
    expect(screen.queryByText(/reconstructed from mix history/i)).not.toBeInTheDocument();
  });

  it('shows a "Removed from library" badge for a track whose trackId is null', async () => {
    const timeline: MixTimelineResponse = {
      outputId: 'output-1',
      available: true,
      tier: 'reconstructed',
      durationSec: 300,
      tracks: [track({ trackId: null, title: 'Unknown track' })],
    };
    apiMock.boogiemix.timeline.mockResolvedValue(timeline);

    render(<MixStoryView output={output()} playTrack={() => {}} onBack={() => {}} onDelete={() => {}} />);

    await waitFor(() => expect(screen.getByText('Removed from library')).toBeInTheDocument());
  });

  it('shows the reconstructed-tier banner but not for a full-tier mix', async () => {
    apiMock.boogiemix.timeline.mockResolvedValue({
      outputId: 'output-1', available: true, tier: 'reconstructed', durationSec: 300, tracks: [track()],
    } as MixTimelineResponse);

    render(<MixStoryView output={output()} playTrack={() => {}} onBack={() => {}} onDelete={() => {}} />);

    await waitFor(() => expect(screen.getByText(/reconstructed from mix history/i)).toBeInTheDocument());
  });

  it('shows an unavailable-tier message instead of a track list or timeline', async () => {
    apiMock.boogiemix.timeline.mockResolvedValue({
      outputId: 'output-1', available: false, tier: 'unavailable', durationSec: 0, tracks: [],
    } as MixTimelineResponse);

    render(<MixStoryView output={output()} playTrack={() => {}} onBack={() => {}} onDelete={() => {}} />);

    await waitFor(() => expect(screen.getByText(/breakdown isn.t available/i)).toBeInTheDocument());
    expect(screen.queryByText('How this mix was built')).not.toBeInTheDocument();
  });

  it('clicking a track row seeks playback to that track\'s start time', async () => {
    const timeline: MixTimelineResponse = {
      outputId: 'output-1',
      available: true,
      tier: 'full',
      durationSec: 600,
      tracks: [track(), track({ stepIndex: 1, trackId: 't2', title: 'Shelter', outputStartSec: 292, outputEndSec: 600 })],
    };
    apiMock.boogiemix.timeline.mockResolvedValue(timeline);
    const playTrack = vi.fn();

    render(<MixStoryView output={output()} playTrack={playTrack} onBack={() => {}} onDelete={() => {}} />);
    await waitFor(() => expect(screen.getByTestId('track-row-1')).toBeInTheDocument());

    fireEvent.click(screen.getByTestId('track-row-1'));

    expect(playTrack).toHaveBeenCalledTimes(1);
    const [playedTrack] = playTrack.mock.calls[0];
    expect(playedTrack.startAtSec).toBe(292);
    expect(playedTrack.stream_url_override).toBe('/api/boogiemix/outputs/output-1/play');
  });

  it('the Play button starts the mix from the beginning (no startAtSec)', async () => {
    apiMock.boogiemix.timeline.mockResolvedValue({
      outputId: 'output-1', available: true, tier: 'full', durationSec: 300, tracks: [track()],
    } as MixTimelineResponse);
    const playTrack = vi.fn();

    render(<MixStoryView output={output()} playTrack={playTrack} onBack={() => {}} onDelete={() => {}} />);
    await waitFor(() => expect(screen.getByTestId('track-row-0')).toBeInTheDocument());

    fireEvent.click(screen.getByRole('button', { name: /^Play Electronic House$/ }));

    expect(playTrack).toHaveBeenCalledTimes(1);
    expect(playTrack.mock.calls[0][0].startAtSec).toBeUndefined();
  });

  it('calls onBack when the breadcrumb is clicked', async () => {
    apiMock.boogiemix.timeline.mockResolvedValue({
      outputId: 'output-1', available: false, tier: 'unavailable', durationSec: 0, tracks: [],
    } as MixTimelineResponse);
    const onBack = vi.fn();

    render(<MixStoryView output={output()} playTrack={() => {}} onBack={onBack} onDelete={() => {}} />);
    await waitFor(() => expect(apiMock.boogiemix.timeline).toHaveBeenCalled());

    fireEvent.click(screen.getByRole('button', { name: /Back to Mixes/i }));
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it('calls onDelete with the output when the delete action is clicked', async () => {
    apiMock.boogiemix.timeline.mockResolvedValue({
      outputId: 'output-1', available: false, tier: 'unavailable', durationSec: 0, tracks: [],
    } as MixTimelineResponse);
    const onDelete = vi.fn();
    const theOutput = output();

    render(<MixStoryView output={theOutput} playTrack={() => {}} onBack={() => {}} onDelete={onDelete} />);
    await waitFor(() => expect(apiMock.boogiemix.timeline).toHaveBeenCalled());

    fireEvent.click(screen.getByRole('button', { name: /^Delete Electronic House$/ }));
    expect(onDelete).toHaveBeenCalledWith(theOutput);
  });

  it('shows the Share image action only when the timeline has real tracks, pointing at the story-image URL', async () => {
    apiMock.boogiemix.timeline.mockResolvedValue({
      outputId: 'output-1', available: true, tier: 'full', durationSec: 300, tracks: [track()],
    } as MixTimelineResponse);

    render(<MixStoryView output={output()} playTrack={() => {}} onBack={() => {}} onDelete={() => {}} />);

    await waitFor(() => expect(screen.getByTitle('Share image')).toBeInTheDocument());
    expect(screen.getByTitle('Share image')).toHaveAttribute('href', '/api/boogiemix/outputs/output-1/story-image');
  });

  it('hides the Share image action for an unavailable-tier mix', async () => {
    apiMock.boogiemix.timeline.mockResolvedValue({
      outputId: 'output-1', available: false, tier: 'unavailable', durationSec: 0, tracks: [],
    } as MixTimelineResponse);

    render(<MixStoryView output={output()} playTrack={() => {}} onBack={() => {}} onDelete={() => {}} />);

    await waitFor(() => expect(apiMock.boogiemix.timeline).toHaveBeenCalled());
    expect(screen.queryByTitle('Share image')).not.toBeInTheDocument();
  });

  it('shows a load-error banner when the timeline fetch rejects', async () => {
    apiMock.boogiemix.timeline.mockRejectedValue(new Error('network error'));

    render(<MixStoryView output={output()} playTrack={() => {}} onBack={() => {}} onDelete={() => {}} />);

    await waitFor(() => expect(screen.getByText(/couldn.t load this mix/i)).toBeInTheDocument());
  });

  it('renders human-readable transition chips between tracks', async () => {
    apiMock.boogiemix.timeline.mockResolvedValue({
      outputId: 'output-1',
      available: true,
      tier: 'full',
      durationSec: 600,
      tracks: [
        track({ transitionOutKind: 'beatmatch', transitionOutPhraseAligned: true, crossfadeOutSec: 8 }),
        track({ stepIndex: 1, trackId: 't2', title: 'Shelter', outputStartSec: 292, outputEndSec: 600, crossfadeOutSec: 0 }),
      ],
    } as MixTimelineResponse);

    render(<MixStoryView output={output()} playTrack={() => {}} onBack={() => {}} onDelete={() => {}} />);

    await waitFor(() => expect(screen.getByText(/Beatmatched.*phrase-aligned.*8s blend/)).toBeInTheDocument());
  });
});

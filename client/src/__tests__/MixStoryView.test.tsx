/**
 * Tests MixStoryView behavior for BoogieBox regressions
 * (wip/boogiemix-story-timeline-plan.md Phase 2).
 */

import React from 'react';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import MixStoryView, { buildSegmentLayout, timeToX, xToTime } from '../components/MixStoryView';
import type { BoogieMixOutput, MixOutputTrackRow, MixTimelineResponse } from '../types';
import type { PlaybackSnapshot } from '../components/Player';

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

  it('renders real album art in the track list and carousel when albumId is present', async () => {
    apiMock.boogiemix.timeline.mockResolvedValue({
      outputId: 'output-1', available: true, tier: 'full', durationSec: 300,
      tracks: [track({ albumId: 'album-1' })],
    } as MixTimelineResponse);

    render(<MixStoryView output={output()} playTrack={() => {}} onBack={() => {}} onDelete={() => {}} />);
    await waitFor(() => expect(screen.getByTestId('track-row-0')).toBeInTheDocument());

    const rowArt = within(screen.getByTestId('track-art-0')).getByRole('presentation');
    expect(rowArt).toHaveAttribute('src', '/api/albums/album-1/art?size=300');

    const segmentArt = within(screen.getByTestId('timeline-segment-0')).getByRole('presentation');
    expect(segmentArt).toHaveAttribute('src', '/api/albums/album-1/art?size=300');
  });

  it('falls back to a color swatch (no <img>) when a track has no albumId', async () => {
    apiMock.boogiemix.timeline.mockResolvedValue({
      outputId: 'output-1', available: true, tier: 'full', durationSec: 300,
      tracks: [track({ albumId: null })],
    } as MixTimelineResponse);

    render(<MixStoryView output={output()} playTrack={() => {}} onBack={() => {}} onDelete={() => {}} />);
    await waitFor(() => expect(screen.getByTestId('track-row-0')).toBeInTheDocument());

    expect(within(screen.getByTestId('track-art-0')).queryByRole('presentation')).not.toBeInTheDocument();
    const artEl = screen.getByTestId('track-art-0') as HTMLElement;
    expect(artEl.style.background).toBeTruthy();
    expect(artEl.style.background).not.toBe('var(--surface-subtle)');
  });

  it('keeps showing real artwork (with a removed badge) for a removed track whose album art still resolves', async () => {
    apiMock.boogiemix.timeline.mockResolvedValue({
      outputId: 'output-1', available: true, tier: 'reconstructed', durationSec: 300,
      tracks: [track({ trackId: null, albumId: 'album-1', title: 'Ghost Town' })],
    } as MixTimelineResponse);

    render(<MixStoryView output={output()} playTrack={() => {}} onBack={() => {}} onDelete={() => {}} />);
    await waitFor(() => expect(screen.getByTestId('track-row-0')).toBeInTheDocument());

    expect(within(screen.getByTestId('track-art-0')).getByRole('presentation')).toHaveAttribute(
      'src', '/api/albums/album-1/art?size=300',
    );
    expect(within(screen.getByTestId('track-art-0')).getByTitle('Removed from library')).toBeInTheDocument();
    expect(screen.getByText('Removed from library')).toBeInTheDocument();
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

  function mockRect(element: HTMLElement, left = 0, width = 360): void {
    vi.spyOn(element, 'getBoundingClientRect').mockReturnValue({
      x: left, y: 0, width, height: 92, top: 0, right: left + width, bottom: 92, left,
      toJSON: () => ({}),
    } as DOMRect);
  }

  function snapshotFor(output_id: string, currentTime: number): PlaybackSnapshot {
    return {
      currentTrack: { id: `boogiemix:${output_id}` } as PlaybackSnapshot['currentTrack'],
      currentTime,
      duration: 600,
      isPlaying: true,
      volume: 1,
      muted: false,
      loading: false,
      audioError: null,
    };
  }

  it('clicking the timeline band seeks proportionally within the clicked track segment', async () => {
    apiMock.boogiemix.timeline.mockResolvedValue({
      outputId: 'output-1',
      available: true,
      tier: 'full',
      durationSec: 600,
      tracks: [track(), track({ stepIndex: 1, trackId: 't2', title: 'Shelter', outputStartSec: 300, outputEndSec: 600 })],
    } as MixTimelineResponse);
    const playTrack = vi.fn();

    render(<MixStoryView output={output()} playTrack={playTrack} onBack={() => {}} onDelete={() => {}} />);
    await waitFor(() => expect(screen.getByTestId('timeline-band')).toBeInTheDocument());

    const band = screen.getByTestId('timeline-band');
    mockRect(band, 0, 360); // two 300s tracks, each floored to a 180px segment (see buildSegmentLayout)

    fireEvent.click(band, { clientX: 90 }); // midpoint of the first segment
    expect(playTrack.mock.calls[0][0].startAtSec).toBeCloseTo(150, 0);

    fireEvent.click(band, { clientX: 270 }); // midpoint of the second segment
    expect(playTrack.mock.calls[1][0].startAtSec).toBeCloseTo(450, 0);
  });

  it('shows the playhead only while this mix is the one actually playing', async () => {
    apiMock.boogiemix.timeline.mockResolvedValue({
      outputId: 'output-1', available: true, tier: 'full', durationSec: 300, tracks: [track()],
    } as MixTimelineResponse);

    const { rerender } = render(
      <MixStoryView output={output()} playTrack={() => {}} onBack={() => {}} onDelete={() => {}} />,
    );
    await waitFor(() => expect(screen.getByTestId('timeline-band')).toBeInTheDocument());
    expect(screen.queryByTestId('playhead')).not.toBeInTheDocument();

    rerender(
      <MixStoryView output={output()} playTrack={() => {}} onBack={() => {}} onDelete={() => {}} playbackSnapshot={snapshotFor('other-output', 50)} />,
    );
    expect(screen.queryByTestId('playhead')).not.toBeInTheDocument();

    rerender(
      <MixStoryView output={output()} playTrack={() => {}} onBack={() => {}} onDelete={() => {}} playbackSnapshot={snapshotFor('output-1', 50)} />,
    );
    expect(screen.getByTestId('playhead')).toBeInTheDocument();
  });

  it('breaks the auto-follow lock on a manual scroll and shows a recenter control', async () => {
    apiMock.boogiemix.timeline.mockResolvedValue({
      outputId: 'output-1', available: true, tier: 'full', durationSec: 600,
      tracks: [track(), track({ stepIndex: 1, trackId: 't2', title: 'Shelter', outputStartSec: 300, outputEndSec: 600 })],
    } as MixTimelineResponse);

    render(
      <MixStoryView
        output={output()}
        playTrack={() => {}}
        onBack={() => {}}
        onDelete={() => {}}
        playbackSnapshot={snapshotFor('output-1', 550)}
      />,
    );
    await waitFor(() => expect(screen.getByTestId('playhead')).toBeInTheDocument());
    expect(screen.queryByTestId('recenter-btn')).not.toBeInTheDocument();

    const scrollEl = screen.getByTestId('timeline-scroll');
    fireEvent.wheel(scrollEl); // a manual gesture breaks the follow lock
    scrollEl.scrollLeft = 0;   // simulate having scrolled away from the (far-along) playhead
    fireEvent.scroll(scrollEl);

    await waitFor(() => expect(screen.getByTestId('recenter-btn')).toBeInTheDocument());

    fireEvent.click(screen.getByTestId('recenter-btn'));
    await waitFor(() => expect(screen.queryByTestId('recenter-btn')).not.toBeInTheDocument());
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

describe('timeline segment layout helpers', () => {
  const tracks: MixOutputTrackRow[] = [
    track(), // 0-300s -> floored to a 180px segment (300 * 0.6)
    track({ stepIndex: 1, trackId: 't2', outputStartSec: 300, outputEndSec: 320 }), // 20s -> floored to the 130px minimum
    track({ stepIndex: 2, trackId: 't3', outputStartSec: 320, outputEndSec: 620 }), // 300s -> 180px
  ];

  it('floors short-track widths to the minimum while scaling longer tracks by duration', () => {
    const { items, totalWidth } = buildSegmentLayout(tracks);
    expect(items.map(i => i.width)).toEqual([180, 130, 180]);
    expect(items.map(i => i.x)).toEqual([0, 180, 310]);
    expect(totalWidth).toBe(490);
  });

  it('maps time to x within the containing segment, not linearly across the whole band', () => {
    const { items } = buildSegmentLayout(tracks);
    expect(timeToX(0, items)).toBe(0);
    expect(timeToX(150, items)).toBeCloseTo(90, 5);   // midpoint of the first (180px) segment
    expect(timeToX(300, items)).toBeCloseTo(180, 5);  // start of the short middle segment
    expect(timeToX(470, items)).toBeCloseTo(400, 5);  // midpoint of the last segment
    expect(timeToX(620, items)).toBe(490);            // clamps to the end
    expect(timeToX(-5, items)).toBe(0);                // clamps before the start
  });

  it('maps x back to time as the exact inverse of timeToX', () => {
    const { items } = buildSegmentLayout(tracks);
    expect(xToTime(0, items)).toBe(0);
    expect(xToTime(90, items)).toBeCloseTo(150, 5);
    expect(xToTime(180, items)).toBeCloseTo(300, 5);
    expect(xToTime(490, items)).toBe(620);
    expect(xToTime(9999, items)).toBe(620); // clamps past the end
  });

  it('handles an empty track list without throwing', () => {
    const { items, totalWidth } = buildSegmentLayout([]);
    expect(items).toEqual([]);
    expect(totalWidth).toBe(0);
    expect(timeToX(10, items)).toBe(0);
    expect(xToTime(10, items)).toBe(0);
  });
});

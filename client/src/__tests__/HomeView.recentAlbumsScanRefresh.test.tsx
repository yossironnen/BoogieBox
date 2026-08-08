/**
 * Tests that the Home Recent Albums carousel refreshes itself while a background scan runs.
 */

import React from 'react';
import { render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import HomeView from '../components/HomeView';
import type { Stats } from '../types';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    latestAlbums: vi.fn(),
    homeTopRated: vi.fn(),
    homeGenres: vi.fn(),
    genres: vi.fn(),
    recentlyPlayed: vi.fn(),
    topPlayedTracks: vi.fn(),
    mostPlayedArtists: vi.fn(),
    scanJobs: {
      active: vi.fn(),
    },
    playlists: {
      list: vi.fn(),
      create: vi.fn(),
    },
    crossfade: {
      config: vi.fn(),
      upsertOverride: vi.fn(),
      removeOverride: vi.fn(),
    },
  },
}));

vi.mock('../api', () => ({ api: apiMock }));

const STATS: Stats = {
  total_tracks: 20,
  total_artists: 5,
  total_albums: 4,
  total_libraries: 1,
  total_hours: 12,
  total_gb: 3.4,
};

function album(id: string, title: string) {
  return { id, title, artist: 'Chromatics', album_artist: 'Chromatics', year: 2026, track_count: 9 };
}

function mockIntersectionObserver(): void {
  class FakeIntersectionObserver {
    observe(): void {}
    disconnect(): void {}
    unobserve(): void {}
  }
  Object.defineProperty(window, 'IntersectionObserver', {
    writable: true,
    configurable: true,
    value: FakeIntersectionObserver,
  });
}

function renderHome() {
  return render(
    <HomeView
      stats={STATS}
      onOpenAlbum={() => {}}
      onOpenArtist={() => {}}
      onOpenGenre={() => {}}
      onBrowseMusic={() => {}}
      onOpenPlaylist={() => {}}
      onPlayTrack={() => {}}
      onStartAutoDj={async () => 0}
    />,
  );
}

describe('HomeView Recent Albums scan refresh', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers({ shouldAdvanceTime: true });
    Object.defineProperty(window, 'matchMedia', {
      writable: true,
      value: vi.fn().mockImplementation((query: string) => ({
        matches: false, media: query, onchange: null,
        addEventListener: vi.fn(), removeEventListener: vi.fn(),
        addListener: vi.fn(), removeListener: vi.fn(), dispatchEvent: vi.fn(),
      })),
    });
    mockIntersectionObserver();
    apiMock.latestAlbums.mockResolvedValue([album('1', 'Night Drive')]);
    apiMock.homeTopRated.mockResolvedValue({ artists: [], albums: [], tracks: [] });
    apiMock.homeGenres.mockResolvedValue([]);
    apiMock.genres.mockResolvedValue([]);
    apiMock.recentlyPlayed.mockResolvedValue([]);
    apiMock.topPlayedTracks.mockResolvedValue([]);
    apiMock.mostPlayedArtists.mockResolvedValue([]);
    apiMock.scanJobs.active.mockResolvedValue([]);
    apiMock.playlists.list.mockResolvedValue([]);
    apiMock.crossfade.config.mockResolvedValue({ mode: 'off', duration: 2, source: 'global' });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('pulls newly scanned albums in while a scan is running', async () => {
    apiMock.scanJobs.active.mockResolvedValue([{ id: 'job-1', status: 'running' }]);
    renderHome();
    await screen.findByTitle(/Night Drive/);

    apiMock.latestAlbums.mockResolvedValue([album('2', 'OutRun'), album('1', 'Night Drive')]);
    await vi.advanceTimersByTimeAsync(5000);

    await waitFor(() => expect(screen.getByTitle(/OutRun/)).toBeInTheDocument());
  });

  it('refreshes once more after the scan finishes, then stops refetching', async () => {
    apiMock.scanJobs.active.mockResolvedValueOnce([{ id: 'job-1', status: 'running' }]);
    renderHome();
    await screen.findByTitle(/Night Drive/);

    await vi.advanceTimersByTimeAsync(5000); // scan active -> refetch
    await vi.advanceTimersByTimeAsync(5000); // scan just ended -> final refetch
    const callsAfterScan = apiMock.latestAlbums.mock.calls.length;

    await vi.advanceTimersByTimeAsync(15000);
    expect(apiMock.latestAlbums.mock.calls.length).toBe(callsAfterScan);
  });

  it('does not poll albums when no scan is running', async () => {
    renderHome();
    await screen.findByTitle(/Night Drive/);
    const initialCalls = apiMock.latestAlbums.mock.calls.length;

    await vi.advanceTimersByTimeAsync(15000);

    expect(apiMock.scanJobs.active).toHaveBeenCalled();
    expect(apiMock.latestAlbums.mock.calls.length).toBe(initialCalls);
  });
});

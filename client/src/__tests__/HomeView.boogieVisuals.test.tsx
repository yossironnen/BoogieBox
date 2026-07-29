/**
 * Tests Home View.Boogie Visuals.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import HomeView from '../components/HomeView';
import type { Stats, Track } from '../types';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    latestAlbums: vi.fn(),
    homeTopRated: vi.fn(),
    homeGenres: vi.fn(),
    genres: vi.fn(),
    recentlyPlayed: vi.fn(),
    topPlayedTracks: vi.fn(),
    mostPlayedArtists: vi.fn(),
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

vi.mock('../api', () => ({
  api: apiMock,
}));

const STATS: Stats = {
  total_tracks: 20,
  total_artists: 5,
  total_albums: 4,
  total_libraries: 1,
  total_hours: 12,
  total_gb: 3.4,
};

const RECENT_TRACKS: Track[] = [
  {
    id: '1',
    file_path: 'a.mp3',
    file_name: 'a.mp3',
    file_size: null,
    format: 'mp3',
    duration: 240,
    bitrate: null,
    sample_rate: null,
    channels: null,
    title: 'After Hours',
    artist: 'Chromatics',
    album: 'Night Drive',
    library_name: null,
    track_number: null,
    disc_number: null,
    year: null,
    genre: null,
    composer: null,
    comment: null,
    bpm: null,
    scanned_at: '2026-03-27 09:00:00',
    last_played_at: '2026-03-27 09:00:00',
    play_count: 8,
  },
  {
    id: '2',
    file_path: 'b.mp3',
    file_name: 'b.mp3',
    file_size: null,
    format: 'mp3',
    duration: 210,
    bitrate: null,
    sample_rate: null,
    channels: null,
    title: 'Nightcall',
    artist: 'Kavinsky',
    album: 'OutRun',
    library_name: null,
    track_number: null,
    disc_number: null,
    year: null,
    genre: null,
    composer: null,
    comment: null,
    bpm: null,
    scanned_at: '2026-03-26 20:00:00',
    last_played_at: '2026-03-26 20:00:00',
    play_count: 6,
  },
];

function mockMatchMedia(reduced: boolean): void {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: query === '(prefers-reduced-motion: reduce)' ? reduced : false,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
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

function renderHome(hybridDesign = false) {
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
      hybridDesign={hybridDesign}
    />,
  );
}

describe('HomeView boogie visuals', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockMatchMedia(false);
    mockIntersectionObserver();
    apiMock.latestAlbums.mockResolvedValue([]);
    apiMock.homeTopRated.mockResolvedValue({ artists: [], albums: [], tracks: [] });
    apiMock.homeGenres.mockResolvedValue([]);
    apiMock.genres.mockResolvedValue([]);
    apiMock.recentlyPlayed.mockResolvedValue(RECENT_TRACKS);
    apiMock.topPlayedTracks.mockResolvedValue([]);
    apiMock.mostPlayedArtists.mockResolvedValue([]);
    apiMock.playlists.list.mockResolvedValue([]);
    apiMock.playlists.create.mockResolvedValue({ id: '1', name: 'Mix' });
    apiMock.crossfade.config.mockResolvedValue({ mode: 'off', duration: 2, source: 'global' });
    apiMock.crossfade.upsertOverride.mockResolvedValue({ ok: true });
    apiMock.crossfade.removeOverride.mockResolvedValue({ ok: true });
  });

  it('disables animation classes when reduced motion is preferred', async () => {
    mockMatchMedia(true);
    renderHome();

    await waitFor(() => expect(screen.getByTestId('boogie-visual-section')).toBeInTheDocument());
    const section = screen.getByTestId('boogie-visual-section');
    const trendLine = document.querySelector('.boogie-trend-line');
    const trendLineClass = trendLine?.getAttribute('class') ?? '';

    expect(section.getAttribute('data-reduced-motion')).toBe('true');
    expect(trendLineClass).not.toContain('boogie-trend-line-animate');
  });

  it('updates transition state when date range changes', async () => {
    renderHome();
    const section = await screen.findByTestId('boogie-visual-section');
    expect(section.getAttribute('data-transition-key')).toBe('0');

    fireEvent.click(screen.getByRole('radio', { name: /last 7 days/i }));

    await waitFor(() => expect(section.getAttribute('data-transition-key')).toBe('1'));
  });

  it('renders the Let\'s Boogie title with enhancement class', async () => {
    renderHome();
    await waitFor(() => expect(screen.getByText("Let's Boogie!")).toBeInTheDocument());
    expect(screen.getByText("Let's Boogie!").className).toContain('boogie-title');
  });

  it('applies the approved Hybrid Home surface without changing its modules', async () => {
    const { container } = renderHome(true);

    await waitFor(() => expect(screen.getByText('Recent Albums')).toBeInTheDocument());
    const root = container.querySelector('[data-ui-design="hybrid"]');
    expect(root).toHaveStyle({ background: 'var(--bg)', padding: '24px 28px 32px' });
    expect(screen.getByText('Library').parentElement?.parentElement).toHaveStyle({
      boxShadow: 'none',
      borderRadius: '16px',
    });
  });

  it('limits the Hybrid Recent Albums hover treatment to the artwork', async () => {
    apiMock.latestAlbums.mockResolvedValue([{
      id: 'album-1',
      title: 'Night Drive',
      artist: 'Chromatics',
      album_artist: 'Chromatics',
      year: 2007,
      genre: 'Electronic',
      track_count: 10,
      added_at: '2026-07-29',
      latest_scanned_at: '2026-07-29',
    }]);
    const { container } = renderHome(true);

    const albumCard = await screen.findByTitle('Night Drive — Chromatics');
    fireEvent.mouseEnter(albumCard);

    expect(albumCard.style.borderColor).toBe('transparent');
    expect(albumCard.style.backgroundColor).toBe('transparent');
    const artwork = container.querySelector('[data-hybrid-recent-album-art="album-1"]') as HTMLElement;
    expect(artwork.style.outline).toContain('var(--accent)');
    expect(artwork.style.filter).toBe('brightness(1.04) saturate(1.05)');
    const overlay = artwork.querySelector('[data-hybrid-art-hover-overlay="recent-album"]') as HTMLElement;
    expect(overlay).toHaveStyle({
      opacity: '1',
      background: 'color-mix(in srgb, var(--accent) 10%, transparent)',
    });
    expect(screen.getByText('Night Drive')).toHaveStyle({ color: 'var(--text)' });
  });
});


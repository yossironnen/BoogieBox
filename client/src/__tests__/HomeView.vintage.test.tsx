/**
 * Tests the Home view under the Vintage Record Shop style: sticker stat tiles and
 * Recent Albums drawn as sleeves with a record whose label is a round crop of the cover.
 */

import React from 'react';
import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import HomeView from '../components/HomeView';
import type { Stats } from '../types';
import { VINTAGE_STYLES, VintageStyleContext } from '../vintageThemes';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    latestAlbums: vi.fn(),
    albumArtUrl: vi.fn((id: string, size: number) => `/api/albums/${id}/art?size=${size}`),
    albumTracks: vi.fn(),
    homeTopRated: vi.fn(),
    homeGenres: vi.fn(),
    genres: vi.fn(),
    recentlyPlayed: vi.fn(),
    topPlayedTracks: vi.fn(),
    mostPlayedArtists: vi.fn(),
    scanJobs: { active: vi.fn() },
    playlists: { list: vi.fn(), create: vi.fn() },
    crossfade: { config: vi.fn(), upsertOverride: vi.fn(), removeOverride: vi.fn() },
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

function renderHome(vintage: boolean) {
  const view = (
    <HomeView
      stats={STATS}
      onOpenAlbum={() => {}}
      onOpenArtist={() => {}}
      onOpenGenre={() => {}}
      onBrowseMusic={() => {}}
      onOpenPlaylist={() => {}}
      onPlayTrack={() => {}}
      onStartAutoDj={async () => 0}
    />
  );
  return render(
    vintage
      ? <VintageStyleContext.Provider value={VINTAGE_STYLES.recordshop}>{view}</VintageStyleContext.Provider>
      : view,
  );
}

describe('HomeView — Vintage Record Shop', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    Object.defineProperty(window, 'matchMedia', {
      writable: true,
      value: vi.fn().mockImplementation((query: string) => ({
        matches: false, media: query, onchange: null,
        addEventListener: vi.fn(), removeEventListener: vi.fn(),
        addListener: vi.fn(), removeListener: vi.fn(), dispatchEvent: vi.fn(),
      })),
    });
    Object.defineProperty(window, 'IntersectionObserver', {
      writable: true,
      configurable: true,
      value: class { observe() {} disconnect() {} unobserve() {} },
    });
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

  it('draws each recent album as a sleeve with a record labelled by a round crop of the cover', async () => {
    const { container } = renderHome(true);
    const tile = await screen.findByTitle(/Night Drive/);

    const record = container.querySelector('[data-vintage-record="1"]') as HTMLElement;
    expect(record).not.toBeNull();
    expect(record).toHaveAttribute('aria-hidden', 'true');
    const label = record.querySelector('[data-vintage-record-label]') as HTMLElement;
    expect(label.style.borderRadius).toBe('50%');
    expect(label.querySelector('img')?.getAttribute('src')).toBe('/api/albums/1/art?size=300');
    expect(apiMock.albumArtUrl).toHaveBeenCalledWith('1', 300);

    expect(record.style.transform).toBe('');
    fireEvent.mouseEnter(tile);
    expect(record.style.transform).toBe('translateX(12px)');
  });

  it('shows the library stats as price-sticker tiles with cycling offset shadows', async () => {
    const { container } = renderHome(true);
    await screen.findByTitle(/Night Drive/);

    const tiles = Array.from(container.querySelectorAll<HTMLElement>('[data-vintage-stat]'));
    expect(tiles.map((t) => t.dataset.vintageStat)).toEqual(['Tracks', 'Artists', 'Albums']);
    const shadows = tiles.map((tile) => tile.style.boxShadow);
    for (const shadow of shadows) expect(shadow).toMatch(/4px 4px 0/);
    // Each sticker gets a different stripe colour.
    expect(new Set(shadows).size).toBe(tiles.length);
  });

  it('keeps the standard look outside Vintage', async () => {
    const { container } = renderHome(false);
    await screen.findByTitle(/Night Drive/);

    expect(container.querySelector('[data-vintage-record]')).toBeNull();
    expect(container.querySelector('[data-vintage-stat]')).toBeNull();
  });
});

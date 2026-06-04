/**
 * Tests Mobile Search View.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import MobileSearchView from './MobileSearchView';
import { api } from '../../api';

vi.mock('../../api', () => ({
  api: {
    search: vi.fn(),
    playlists: {
      list: vi.fn(),
      addTrack: vi.fn(),
      create: vi.fn(),
    },
    setTrackRating: vi.fn(),
  },
}));

describe('MobileSearchView', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.mocked(api.search).mockReset();
    vi.mocked(api.playlists.list).mockResolvedValue([{ id: '1', name: 'Pocket Mix', track_count: 4, total_duration: 1000, created_at: '2026-01-01', updated_at: '2026-01-01', description: null }]);
    vi.mocked(api.playlists.addTrack).mockResolvedValue({ ok: true });
    vi.mocked(api.playlists.create).mockResolvedValue({ id: '2', name: 'Fresh Picks', track_count: 0, total_duration: 0, created_at: '2026-01-01', updated_at: '2026-01-01', description: null });
    vi.mocked(api.setTrackRating).mockResolvedValue({ ok: true, rating: null });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('waits for a minimum query length before searching', () => {
    render(<MobileSearchView onPlayTrack={vi.fn()} onAddToQueue={vi.fn()} />);
    fireEvent.change(screen.getByPlaceholderText(/songs, artists, albums/i), { target: { value: 'a' } });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    expect(api.search).not.toHaveBeenCalled();
    expect(screen.getByText(/keep typing to search your library/i)).toBeInTheDocument();
  });

  it('uses the mobile tracks-only search mode', async () => {
    vi.mocked(api.search).mockResolvedValue({
      tracks: [],
      artists: [],
      albums: [],
      top_results: [],
      total: 0,
      page: 1,
      limit: 20,
      hasMore: false,
    });

    render(<MobileSearchView onPlayTrack={vi.fn()} onAddToQueue={vi.fn()} />);
    fireEvent.change(screen.getByPlaceholderText(/songs, artists, albums/i), { target: { value: 'abba' } });
    await act(async () => {
      vi.advanceTimersByTime(400);
      await Promise.resolve();
    });

    expect(api.search).toHaveBeenCalledWith({
      q: 'abba',
      limit: 20,
      page: 1,
      search_mode: 'mobile_omni',
      mode: 'music',
      include_artists: true,
      include_albums: true,
      include_total: true,
      sort: 'relevance',
      track_rating_filter: 'all',
      album_rating_filter: 'all',
      artist_rating_filter: 'all',
      year: undefined,
    });
  });

  it('opens the kebab sheet and keeps row taps separate from track actions', async () => {
    vi.useRealTimers();
    const onPlayTrack = vi.fn();
    const onAddToQueue = vi.fn();
    vi.mocked(api.search).mockResolvedValue({
      tracks: [{
        id: '44',
        file_path: 'x',
        file_name: 'anthem.mp3',
        file_size: 1,
        format: 'MP3',
        duration: 120,
        bitrate: 320,
        sample_rate: 44100,
        channels: 2,
        title: 'Anthem',
        artist: 'Artist',
        album: 'Album',
        library_name: 'Main',
        track_number: 1,
        disc_number: 1,
        year: 2025,
        genre: 'Pop',
        composer: null,
        comment: null,
        bpm: null,
        scanned_at: '2026-01-01',
      }],
      artists: [],
      albums: [],
      top_results: [],
      total: 1,
      page: 1,
      limit: 20,
      hasMore: false,
    });

    render(<MobileSearchView onPlayTrack={onPlayTrack} onAddToQueue={onAddToQueue} />);
    fireEvent.change(screen.getByPlaceholderText(/songs, artists, albums/i), { target: { value: 'abba' } });

    await waitFor(() => expect(api.search).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByRole('button', { name: 'More actions for Anthem' })).toBeInTheDocument());
    fireEvent.click(screen.getByRole('button', { name: 'More actions for Anthem' }));
    fireEvent.click(screen.getByRole('button', { name: 'Add To Queue' }));
    expect(onAddToQueue).toHaveBeenCalledTimes(1);
    expect(onPlayTrack).not.toHaveBeenCalled();

    fireEvent.click(screen.getByText('Anthem'));
    expect(onPlayTrack).toHaveBeenCalledTimes(1);
  });

  it('creates a playlist from the search picker flow and adds the selected track', async () => {
    vi.useRealTimers();
    vi.mocked(api.search).mockResolvedValue({
      tracks: [{
        id: '44',
        file_path: 'x',
        file_name: 'anthem.mp3',
        file_size: 1,
        format: 'MP3',
        duration: 120,
        bitrate: 320,
        sample_rate: 44100,
        channels: 2,
        title: 'Anthem',
        artist: 'Artist',
        album: 'Album',
        library_name: 'Main',
        track_number: 1,
        disc_number: 1,
        year: 2025,
        genre: 'Pop',
        composer: null,
        comment: null,
        bpm: null,
        scanned_at: '2026-01-01',
      }],
      artists: [],
      albums: [],
      top_results: [],
      total: 1,
      page: 1,
      limit: 20,
      hasMore: false,
    });

    render(<MobileSearchView onPlayTrack={vi.fn()} onAddToQueue={vi.fn()} />);
    fireEvent.change(screen.getByPlaceholderText(/songs, artists, albums/i), { target: { value: 'abba' } });

    await waitFor(() => expect(api.search).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByRole('button', { name: 'More actions for Anthem' })).toBeInTheDocument());
    fireEvent.click(screen.getByRole('button', { name: 'More actions for Anthem' }));
    fireEvent.click(screen.getByRole('button', { name: 'Add To Playlist' }));
    fireEvent.click(screen.getByRole('button', { name: 'New Playlist' }));
    fireEvent.change(screen.getByPlaceholderText('Late-night mix'), { target: { value: 'Fresh Picks' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create playlist' }));

    await waitFor(() => expect(api.playlists.create).toHaveBeenCalledWith('Fresh Picks', ''));
    await waitFor(() => expect(api.playlists.addTrack).toHaveBeenCalledWith('2', '44'));
  });

  it('renders music-only top results in mobile search', async () => {
    vi.useRealTimers();
    vi.mocked(api.search).mockResolvedValue({
      tracks: [],
      artists: [],
      albums: [],
      top_results: [{ type: 'album', id: 'm1', title: 'Arrival', subtitle: '2016 · Sci-Fi', score: 96 }],
      total: 0,
      page: 1,
      limit: 20,
      hasMore: false,
    });

    render(<MobileSearchView onPlayTrack={vi.fn()} onAddToQueue={vi.fn()} />);
    fireEvent.change(screen.getByPlaceholderText(/songs, artists, albums/i), { target: { value: 'arr' } });
    await waitFor(() => expect(api.search).toHaveBeenCalled());
    expect(screen.getByText('Top Results')).toBeInTheDocument();
    expect(screen.queryByText('Movies')).not.toBeInTheDocument();
    expect(screen.queryByText('TV Shows')).not.toBeInTheDocument();
    expect(screen.getByText('Arrival')).toBeInTheDocument();
  });
});

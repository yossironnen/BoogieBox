import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import FavouritesView, { formatDuration, visibleFavourites } from './FavouritesView';
import FavouritesWidget from './FavouritesWidget';
import { HeartToggle } from './FavouriteIcons';
import { favouritesStore } from '../favourites';
import type { Album, Artist, Track } from '../types';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    favourites: { ids: vi.fn(), list: vi.fn(), add: vi.fn(), remove: vi.fn() },
    albumArtUrl: vi.fn(() => '/art.jpg'),
    artistPhotoUrl: vi.fn(() => '/photo.jpg'),
    playlists: { list: vi.fn().mockResolvedValue([]) },
  },
}));
vi.mock('../api', () => ({ api: apiMock }));

const track = (id: string, title: string): Track => ({
  id, file_name: `${id}.mp3`, file_size: 1, format: 'mp3', duration: 125, bitrate: null, sample_rate: null,
  channels: null, title, artist: 'The Band', album: 'First', library_name: null, track_number: 1, disc_number: 1,
  year: null, genre: null, composer: null, comment: null, bpm: null, album_id: 'al1', scanned_at: '',
});
const album: Album = { id: 'al1', title: 'First', artist: 'The Band', album_artist: 'The Band', year: 2001, genre: null, track_count: 9 };
const artist: Artist = { id: 'ar1', name: 'The Band', track_count: 9, album_count: 1 };
const data = { tracks: [track('t1', 'Opening'), track('t2', 'Closing')], albums: [album], artists: [artist] };

async function seedStore() {
  apiMock.favourites.ids.mockResolvedValue({ tracks: ['t1', 't2'], albums: ['al1'], artists: ['ar1'] });
  await favouritesStore.load(true);
}

describe('FavouritesView', () => {
  beforeEach(async () => {
    Object.values(apiMock.favourites).forEach((m) => m.mockReset());
    apiMock.favourites.list.mockResolvedValue(data);
    apiMock.favourites.add.mockResolvedValue({ ok: true });
    apiMock.favourites.remove.mockResolvedValue({ ok: true });
    favouritesStore.reset();
    await seedStore();
  });

  it('formats durations and filters out un-hearted rows', () => {
    expect(formatDuration(125)).toBe('2:05');
    expect(formatDuration(null)).toBe('');
    const shown = visibleFavourites(data, (kind, id) => kind === 'track' && id === 't2');
    expect(shown.tracks.map((t) => t.id)).toEqual(['t2']);
    expect(shown.albums).toEqual([]);
  });

  it('plays songs, opens albums and artists, and un-hearts in place', async () => {
    const playTrack = vi.fn();
    const onOpenAlbum = vi.fn();
    const onOpenArtist = vi.fn();
    const onPlayAlbum = vi.fn();
    const onPlayArtist = vi.fn();
    render(
      <FavouritesView
        playTrack={playTrack}
        addToQueue={vi.fn()}
        onOpenAlbum={onOpenAlbum}
        onOpenArtist={onOpenArtist}
        onPlayAlbum={onPlayAlbum}
        onPlayArtist={onPlayArtist}
      />,
    );
    expect(await screen.findByText('Opening')).toBeInTheDocument();
    expect(screen.getAllByText('2:05')).toHaveLength(2);
    fireEvent.click(screen.getByRole('button', { name: 'Play all favourite songs' }));
    expect(playTrack).toHaveBeenCalledWith(data.tracks[0], data.tracks, { type: 'favourites', id: '0' });
    fireEvent.click(screen.getByRole('button', { name: 'Play Closing' }));
    expect(playTrack).toHaveBeenLastCalledWith(data.tracks[1], data.tracks, { type: 'favourites', id: '0' });

    fireEvent.click(screen.getByRole('button', { name: 'Remove Opening from Favourites' }));
    await waitFor(() => expect(screen.queryByText('Opening')).not.toBeInTheDocument());
    expect(apiMock.favourites.remove).toHaveBeenCalledWith('track', 't1');

    fireEvent.click(screen.getByRole('button', { name: /Albums/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Play First' }));
    expect(onPlayAlbum).toHaveBeenCalledWith(album);
    fireEvent.click(screen.getByText('First'));
    expect(onOpenAlbum).toHaveBeenCalledWith(album);

    fireEvent.click(screen.getByRole('button', { name: /Artists/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Play The Band radio' }));
    expect(onPlayArtist).toHaveBeenCalledWith(artist);
    fireEvent.keyDown(screen.getAllByRole('button', { name: /The Band/ })[0], { key: 'Enter' });
    expect(onOpenArtist).toHaveBeenCalledWith(artist);
  });

  it('shows empty sections and load errors', async () => {
    apiMock.favourites.list.mockResolvedValueOnce({ tracks: [], albums: [], artists: [] });
    const { unmount } = render(
      <FavouritesView playTrack={vi.fn()} addToQueue={vi.fn()} onOpenAlbum={vi.fn()} onOpenArtist={vi.fn()} onPlayAlbum={vi.fn()} onPlayArtist={vi.fn()} initialSection="albums" />,
    );
    expect(await screen.findByText('No favourite albums yet')).toBeInTheDocument();
    unmount();
    apiMock.favourites.list.mockRejectedValueOnce(new Error('Server down'));
    render(
      <FavouritesView playTrack={vi.fn()} addToQueue={vi.fn()} onOpenAlbum={vi.fn()} onOpenArtist={vi.fn()} onPlayAlbum={vi.fn()} onPlayArtist={vi.fn()} />,
    );
    expect(await screen.findByRole('alert')).toHaveTextContent('Server down');
  });
});

describe('FavouritesWidget', () => {
  beforeEach(async () => {
    Object.values(apiMock.favourites).forEach((m) => m.mockReset());
    apiMock.favourites.list.mockResolvedValue(data);
    apiMock.favourites.remove.mockResolvedValue({ ok: true });
    favouritesStore.reset();
    await seedStore();
  });

  it('shows counts, latest albums and songs, and opens Favourites', async () => {
    const onOpenFavourites = vi.fn();
    const onOpenAlbum = vi.fn();
    const onPlayTrack = vi.fn();
    render(<FavouritesWidget onOpenAlbum={onOpenAlbum} onPlayTrack={onPlayTrack} onOpenFavourites={onOpenFavourites} />);
    expect(await screen.findByText('Opening')).toBeInTheDocument();
    expect(screen.getByText('2 songs')).toBeInTheDocument();
    expect(screen.getByText('1 albums')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Open Favourites' }));
    expect(onOpenFavourites).toHaveBeenCalled();
    fireEvent.click(screen.getByTitle('First'));
    expect(onOpenAlbum).toHaveBeenCalledWith(album);
    fireEvent.click(screen.getByRole('button', { name: 'Play Opening' }));
    expect(onPlayTrack).toHaveBeenCalledWith(data.tracks[0], data.tracks, { type: 'favourites', id: '0' });
  });

  it('explains how to add favourites when there are none', async () => {
    apiMock.favourites.list.mockResolvedValue({ tracks: [], albums: [], artists: [] });
    favouritesStore.reset();
    render(<FavouritesWidget onOpenAlbum={vi.fn()} onPlayTrack={vi.fn()} onOpenFavourites={vi.fn()} />);
    expect(await screen.findByText(/Add to Favourites/)).toBeInTheDocument();
  });

  it('refreshes after a heart changes elsewhere', async () => {
    vi.useFakeTimers();
    try {
      render(<FavouritesWidget onOpenAlbum={vi.fn()} onPlayTrack={vi.fn()} onOpenFavourites={vi.fn()} />);
      await act(async () => { await vi.advanceTimersByTimeAsync(10); });
      expect(apiMock.favourites.list).toHaveBeenCalledTimes(1);
      await act(async () => {
        await favouritesStore.set('track', 't1', false);
        await vi.advanceTimersByTimeAsync(500);
      });
      expect(apiMock.favourites.list).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe('HeartToggle', () => {
  it('labels and toggles without bubbling', () => {
    const onToggle = vi.fn();
    const parent = vi.fn();
    render(<div onClick={parent}><HeartToggle on={false} label="Song" onToggle={onToggle} /></div>);
    const button = screen.getByRole('button', { name: 'Add Song to Favourites' });
    expect(button).toHaveAttribute('aria-pressed', 'false');
    fireEvent.click(button);
    expect(onToggle).toHaveBeenCalled();
    expect(parent).not.toHaveBeenCalled();
  });
});

describe('FavouritesView fallbacks', () => {
  beforeEach(async () => {
    Object.values(apiMock.favourites).forEach((m) => m.mockReset());
    apiMock.favourites.ids.mockResolvedValue({ tracks: ['t5'], albums: ['al5'], artists: ['ar5'] });
    apiMock.favourites.add.mockResolvedValue({ ok: true });
    apiMock.favourites.remove.mockResolvedValue({ ok: true });
    favouritesStore.reset();
    await favouritesStore.load(true);
  });

  it('uses file names and blank artists, and toggles album and artist hearts', async () => {
    const untitled = { ...track('t5', ''), title: null, artist: null, album: null, duration: null };
    const bare: Album = { ...album, id: 'al5', album_artist: null, artist: null };
    const solo: Artist = { ...artist, id: 'ar5', name: 'Solo' };
    apiMock.favourites.list.mockResolvedValue({ tracks: [untitled], albums: [bare], artists: [solo] });
    const onOpenAlbum = vi.fn();
    render(
      <FavouritesView playTrack={vi.fn()} addToQueue={vi.fn()} onOpenAlbum={onOpenAlbum} onOpenArtist={vi.fn()} onPlayAlbum={vi.fn()} onPlayArtist={vi.fn()} />,
    );
    expect(await screen.findByText('t5.mp3')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /Albums/ }));
    fireEvent.keyDown(screen.getByText('First').closest('[role="button"]')!, { key: 'Enter' });
    expect(onOpenAlbum).toHaveBeenCalledWith(bare);
    fireEvent.click(screen.getByRole('button', { name: 'Remove First from Favourites' }));
    await waitFor(() => expect(apiMock.favourites.remove).toHaveBeenCalledWith('album', 'al5'));
    fireEvent.click(screen.getByRole('button', { name: /Artists/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Remove Solo from Favourites' }));
    await waitFor(() => expect(apiMock.favourites.remove).toHaveBeenCalledWith('artist', 'ar5'));
    expect(await screen.findByText('No favourite artists yet')).toBeInTheDocument();
  });

  it('shows a generic message for non-Error failures', async () => {
    apiMock.favourites.list.mockRejectedValue('boom');
    render(
      <FavouritesView playTrack={vi.fn()} addToQueue={vi.fn()} onOpenAlbum={vi.fn()} onOpenArtist={vi.fn()} onPlayAlbum={vi.fn()} onPlayArtist={vi.fn()} />,
    );
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not load favourites');
  });
});

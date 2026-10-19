import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { MobileFavouritesSection, MobileRadioSection } from './MobileLibraryExtras';
import { favouritesStore } from '../../favourites';
import type { Track } from '../../types';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    favourites: { ids: vi.fn(), list: vi.fn(), add: vi.fn(), remove: vi.fn() },
    radioStations: { list: vi.fn() },
    albumArtUrl: vi.fn(() => '/art.jpg'),
    artistPhotoUrl: vi.fn(() => '/photo.jpg'),
  },
}));
vi.mock('../../api', () => ({ api: apiMock }));

const song: Track = {
  id: 't1', file_name: 't1.mp3', file_size: 1, format: 'mp3', duration: 100, bitrate: null, sample_rate: null,
  channels: null, title: 'Opening', artist: 'The Band', album: 'First', library_name: null, track_number: 1, disc_number: 1,
  year: null, genre: null, composer: null, comment: null, bpm: null, album_id: 'al1', scanned_at: '',
};
const album = { id: 'al1', title: 'First', artist: 'The Band', album_artist: 'The Band', year: 2001, genre: null, track_count: 9 };
const artist = { id: 'ar1', name: 'The Band', track_count: 9, album_count: 1 };

describe('MobileFavouritesSection', () => {
  beforeEach(async () => {
    Object.values(apiMock.favourites).forEach((m) => m.mockReset());
    apiMock.favourites.ids.mockResolvedValue({ tracks: ['t1'], albums: ['al1'], artists: ['ar1'] });
    apiMock.favourites.remove.mockResolvedValue({ ok: true });
    favouritesStore.reset();
    await favouritesStore.load(true);
  });

  it('plays, opens and un-hearts favourites', async () => {
    apiMock.favourites.list.mockResolvedValue({ tracks: [song], albums: [album], artists: [artist] });
    const onPlayTrack = vi.fn();
    const onOpenAlbum = vi.fn();
    const onOpenArtist = vi.fn();
    const onTrackActions = vi.fn();
    render(<MobileFavouritesSection onPlayTrack={onPlayTrack} onOpenAlbum={onOpenAlbum} onOpenArtist={onOpenArtist} onTrackActions={onTrackActions} />);
    fireEvent.click(await screen.findByText('Opening'));
    expect(onPlayTrack).toHaveBeenCalledWith(song, [song], { type: 'favourites', id: '0' });
    fireEvent.click(screen.getByRole('button', { name: 'More actions for Opening' }));
    expect(onTrackActions).toHaveBeenCalledWith(song, [song]);
    fireEvent.click(screen.getByText('First'));
    expect(onOpenAlbum).toHaveBeenCalledWith(album);
    fireEvent.click(screen.getByText('1 albums • 9 tracks'));
    expect(onOpenArtist).toHaveBeenCalledWith(artist);
    fireEvent.click(screen.getByRole('button', { name: 'Remove Opening from Favourites' }));
    await waitFor(() => expect(screen.queryByText('Opening')).not.toBeInTheDocument());
  });

  it('shows empty and error states', async () => {
    apiMock.favourites.list.mockResolvedValueOnce({ tracks: [], albums: [], artists: [] });
    const props = { onPlayTrack: vi.fn(), onOpenAlbum: vi.fn(), onOpenArtist: vi.fn(), onTrackActions: vi.fn() };
    const { unmount } = render(<MobileFavouritesSection {...props} />);
    expect(await screen.findByText(/Add songs, albums and artists/)).toBeInTheDocument();
    unmount();
    apiMock.favourites.list.mockRejectedValueOnce(new Error('x'));
    render(<MobileFavouritesSection {...props} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('could not be loaded');
  });
});

describe('MobileRadioSection', () => {
  beforeEach(() => apiMock.radioStations.list.mockReset());

  it('plays stations and marks the playing one', async () => {
    const station = { id: 's1', name: 'Jazz FM', streamUrl: 'http://j/s', homepageUrl: null };
    apiMock.radioStations.list.mockResolvedValue([station]);
    const onPlayStation = vi.fn();
    render(<MobileRadioSection onPlayStation={onPlayStation} playingStationId="s1" />);
    fireEvent.click(await screen.findByText('Jazz FM'));
    expect(onPlayStation).toHaveBeenCalledWith(station);
    expect(screen.getByText('Playing')).toBeInTheDocument();
  });

  it('shows empty and error states', async () => {
    apiMock.radioStations.list.mockResolvedValueOnce([]);
    const { unmount } = render(<MobileRadioSection onPlayStation={vi.fn()} />);
    expect(await screen.findByText('No radio stations yet.')).toBeInTheDocument();
    unmount();
    apiMock.radioStations.list.mockRejectedValueOnce(new Error('x'));
    render(<MobileRadioSection onPlayStation={vi.fn()} />);
    expect(await screen.findByRole('alert')).toBeInTheDocument();
  });
});

describe('MobileLibraryExtras fallbacks', () => {
  beforeEach(async () => {
    Object.values(apiMock.favourites).forEach((m) => m.mockReset());
    apiMock.favourites.ids.mockResolvedValue({ tracks: ['t9'], albums: ['al9'], artists: [] });
    apiMock.favourites.add.mockResolvedValue({ ok: true });
    favouritesStore.reset();
    await favouritesStore.load(true);
  });

  it('falls back to file names and unknown artists', async () => {
    const untitled = { ...song, id: 't9', title: null, artist: null };
    const bare = { ...album, id: 'al9', album_artist: null, artist: null };
    apiMock.favourites.list.mockResolvedValue({ tracks: [untitled], albums: [bare], artists: [] });
    render(<MobileFavouritesSection onPlayTrack={vi.fn()} onOpenAlbum={vi.fn()} onOpenArtist={vi.fn()} onTrackActions={vi.fn()} />);
    expect(await screen.findByText('t1.mp3')).toBeInTheDocument();
    expect(screen.getByText('Unknown artist')).toBeInTheDocument();
    expect(screen.getByLabelText('Favourite albums')).toBeInTheDocument();
    expect(screen.queryByLabelText('Favourite artists')).not.toBeInTheDocument();
  });

  it('marks only the playing station', async () => {
    apiMock.radioStations.list.mockResolvedValue([
      { id: 's1', name: 'One', streamUrl: 'http://a/s', homepageUrl: null },
      { id: 's2', name: 'Two', streamUrl: 'http://b/s', homepageUrl: null },
    ]);
    render(<MobileRadioSection onPlayStation={vi.fn()} />);
    expect(await screen.findByText('One')).toBeInTheDocument();
    expect(screen.getAllByText('Play')).toHaveLength(2);
  });
});

import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import MobileHomeView from './MobileHomeView';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    latestAlbums: vi.fn(),
    homeTopRated: vi.fn(),
    recentlyPlayed: vi.fn(),
    topPlayedTracks: vi.fn(),
    albumArtUrl: vi.fn((id: string) => `/art/${id}`),
    playlists: { list: vi.fn() },
  },
}));

vi.mock('../../api', () => ({ api: apiMock }));
vi.mock('../../components/ArtImage', () => ({
  default: ({ src }: { src: string }) => <img src={src} alt="" />,
}));

const track = (id: string, overrides = {}) => ({
  id,
  file_path: `${id}.mp3`,
  file_name: `${id}.mp3`,
  title: `Track ${id}`,
  artist: 'Artist',
  album: 'Album',
  album_id: 'a1',
  duration: 120,
  ...overrides,
});

describe('MobileHomeView', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    apiMock.latestAlbums.mockResolvedValue([
      { id: 'a1', title: 'New Album', album_artist: 'Album Artist', artist: null },
      { id: 'a2', title: 'Mystery Album', album_artist: '', artist: '' },
    ]);
    apiMock.homeTopRated.mockResolvedValue({
      artists: [{ id: 'r1', name: 'Rated Artist' }],
      albums: [{ id: 'a3', title: 'Rated Album', album_artist: '', artist: 'Rated Artist' }],
      tracks: [track('rated'), track('fallback', { title: '', artist: '' })],
    });
    apiMock.playlists.list.mockResolvedValue([
      { id: 'p1', name: 'Favorites', track_count: 3, art_album_ids: ['playlist-art'] },
      { id: 'p2', name: 'Empty', track_count: null },
      { id: 'p3', name: 'Third' },
      { id: 'p4', name: 'Fourth' },
      { id: 'p5', name: 'Hidden Fifth' },
    ]);
    apiMock.recentlyPlayed.mockResolvedValue([
      track('recent'),
      track('bare', { title: '', artist: '', album_id: null }),
    ]);
    apiMock.topPlayedTracks.mockResolvedValue([track('top')]);
  });

  it('loads every section and routes album, playlist, browse, and track actions', async () => {
    const onOpenAlbum = vi.fn();
    const onOpenPlaylist = vi.fn();
    const onOpenBrowse = vi.fn();
    const onPlayTrack = vi.fn();
    render(
      <MobileHomeView
        onOpenAlbum={onOpenAlbum}
        onOpenPlaylist={onOpenPlaylist}
        onOpenBrowse={onOpenBrowse}
        onPlayTrack={onPlayTrack}
      />,
    );

    expect(screen.getByRole('heading', { name: 'What will you play next?' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Refresh' })).toBeDisabled();
    expect(await screen.findByText('Recently Added Music')).toBeInTheDocument();
    expect(screen.getByText('Top Rated')).toBeInTheDocument();
    expect(screen.getByText('Your Playlists')).toBeInTheDocument();
    expect(screen.getByText('Recently Played')).toBeInTheDocument();
    expect(screen.getByText('Top Played Tracks')).toBeInTheDocument();
    expect(screen.queryByText('Hidden Fifth')).not.toBeInTheDocument();
    expect(screen.getByLabelText('Recently added albums')).toBeInTheDocument();
    expect(screen.getByLabelText('Recently played tracks')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Favorites/i })).toHaveStyle({ minHeight: '66px' });
    expect(apiMock.albumArtUrl).toHaveBeenCalledWith('playlist-art', 300);

    fireEvent.click(screen.getByRole('button', { name: /New Album/i }));
    fireEvent.click(screen.getByRole('button', { name: /Rated Album/i }));
    expect(onOpenAlbum).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByRole('button', { name: /Favorites/i }));
    expect(onOpenPlaylist).toHaveBeenCalledWith('p1');
    fireEvent.click(screen.getByRole('button', { name: 'See all' }));
    expect(onOpenBrowse).toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: /Track rated/i }));
    fireEvent.click(screen.getByRole('button', { name: /Track recent/i }));
    fireEvent.click(screen.getByRole('button', { name: /Track top/i }));
    expect(onPlayTrack).toHaveBeenCalledTimes(3);

    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
    await waitFor(() => expect(apiMock.latestAlbums).toHaveBeenCalledTimes(2));
    expect(apiMock.latestAlbums).toHaveBeenLastCalledWith(8);
    expect(apiMock.homeTopRated).toHaveBeenLastCalledWith(5);
  });

  it('settles partial and total API failures without rendering empty sections', async () => {
    apiMock.latestAlbums.mockRejectedValue(new Error('offline'));
    apiMock.homeTopRated.mockResolvedValue({ artists: [], albums: [], tracks: [] });
    apiMock.playlists.list.mockRejectedValue(new Error('offline'));
    apiMock.recentlyPlayed.mockResolvedValue([]);
    apiMock.topPlayedTracks.mockRejectedValue(new Error('offline'));
    const { unmount } = render(
      <MobileHomeView
        onOpenAlbum={vi.fn()}
        onOpenPlaylist={vi.fn()}
        onOpenBrowse={vi.fn()}
        onPlayTrack={vi.fn()}
      />,
    );
    await waitFor(() => expect(screen.queryByText('Recently Added Music')).not.toBeInTheDocument());
    expect(screen.queryByText('Top Rated')).not.toBeInTheDocument();
    expect(screen.queryByText('Your Playlists')).not.toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveTextContent('Home is quiet right now');
    unmount();
  });
});

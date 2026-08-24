import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import TrackInfoModal from './TrackInfoModal';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    track: vi.fn(),
    genres: vi.fn(),
    updateTrackMetadata: vi.fn(),
    artists: vi.fn(),
    getArtistMergeInfo: vi.fn(),
  },
}));

vi.mock('../api', () => ({ api: apiMock }));

const track = {
  id: 'track-1',
  file_path: 'D:\\Music\\deadmau5\\Strobe.flac',
  file_name: '02 - Strobe.flac',
  file_size: 82246041,
  format: 'flac',
  duration: 637,
  bitrate: 1032,
  sample_rate: 44100,
  channels: 2,
  title: 'Strobe',
  artist: 'deadmau5',
  album: 'For Lack of a Better Name',
  library_name: 'Home Library',
  track_number: 2,
  disc_number: 1,
  year: 2009,
  genre: 'Progressive House',
  composer: 'Joel Zimmerman',
  comment: '',
  bpm: 128,
  bpm_detected: 127.8,
  bpm_source: 'neural',
  bpm_confidence: 0.92,
  scanned_at: '2026-08-10 14:22:00',
  last_played_at: '2026-08-15 21:03:00',
  play_count: 47,
} as any;

describe('TrackInfoModal', () => {
  beforeEach(() => {
    Object.values(apiMock).forEach(mock => mock.mockReset());
    apiMock.track.mockResolvedValue(track);
    apiMock.genres.mockResolvedValue([{ genre: 'Progressive House' }, { genre: 'Techno' }]);
    apiMock.updateTrackMetadata.mockResolvedValue({ ok: true });
    apiMock.artists.mockResolvedValue([]);
    apiMock.getArtistMergeInfo.mockResolvedValue({ merged: false, members: [] });
  });

  it('loads and displays read-only track detail', async () => {
    render(<TrackInfoModal trackId="track-1" onClose={vi.fn()} onSaved={vi.fn()} />);

    expect(await screen.findByText('D:\\Music\\deadmau5\\Strobe.flac')).toBeInTheDocument();
    expect(screen.getByText('flac')).toBeInTheDocument();
    expect(screen.getByText('Home Library')).toBeInTheDocument();
    expect(screen.getByText('47')).toBeInTheDocument();
    expect(screen.getByText('neural · 92% conf.')).toBeInTheDocument();
  });

  it('edits and saves metadata fields', async () => {
    const onSaved = vi.fn();
    const onClose = vi.fn();
    render(<TrackInfoModal trackId="track-1" onClose={onClose} onSaved={onSaved} />);
    await screen.findByDisplayValue('Strobe');

    fireEvent.change(screen.getByDisplayValue('Strobe'), { target: { value: ' Strobe (Remaster) ' } });
    fireEvent.change(screen.getByDisplayValue('Progressive House'), { target: { value: 'House' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(apiMock.updateTrackMetadata).toHaveBeenCalled());
    expect(apiMock.updateTrackMetadata).toHaveBeenCalledWith('track-1', expect.objectContaining({
      title: 'Strobe (Remaster)',
      genre: 'House',
    }));
    expect(onSaved).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it('requires a non-empty title', async () => {
    render(<TrackInfoModal trackId="track-1" onClose={vi.fn()} onSaved={vi.fn()} />);
    fireEvent.change(await screen.findByDisplayValue('Strobe'), { target: { value: '   ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(screen.getByText('Title is required')).toBeInTheDocument();
    expect(apiMock.updateTrackMetadata).not.toHaveBeenCalled();
  });

  it('shows a save error from the server (e.g. permission denied)', async () => {
    apiMock.updateTrackMetadata.mockRejectedValueOnce(new Error('Forbidden'));
    render(<TrackInfoModal trackId="track-1" onClose={vi.fn()} onSaved={vi.fn()} />);
    await screen.findByDisplayValue('Strobe');

    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('Forbidden')).toBeInTheDocument();
  });

  it('hides the path row when file_path is not present', async () => {
    apiMock.track.mockResolvedValue({ ...track, file_path: null });
    render(<TrackInfoModal trackId="track-1" onClose={vi.fn()} onSaved={vi.fn()} />);

    await screen.findByDisplayValue('Strobe');
    expect(screen.queryByText(/D:\\Music/)).not.toBeInTheDocument();
  });

  it('falls back to "Never" and 0 for a never-played track', async () => {
    apiMock.track.mockResolvedValue({ ...track, last_played_at: null, play_count: null });
    render(<TrackInfoModal trackId="track-1" onClose={vi.fn()} onSaved={vi.fn()} />);

    await screen.findByDisplayValue('Strobe');
    expect(screen.getByText('Never')).toBeInTheDocument();
    expect(screen.getByText('0')).toBeInTheDocument();
  });

  it('copies the file path to the clipboard', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(<TrackInfoModal trackId="track-1" onClose={vi.fn()} onSaved={vi.fn()} />);

    await screen.findByText('D:\\Music\\deadmau5\\Strobe.flac');
    fireEvent.click(screen.getByRole('button', { name: /Copy/ }));
    expect(writeText).toHaveBeenCalledWith('D:\\Music\\deadmau5\\Strobe.flac');
    expect(await screen.findByRole('button', { name: /Copied/ })).toBeInTheDocument();
  });

  it('closes on Escape and backdrop click, but not on inner clicks', async () => {
    const onClose = vi.fn();
    const { container } = render(<TrackInfoModal trackId="track-1" onClose={onClose} onSaved={vi.fn()} />);
    await screen.findByDisplayValue('Strobe');

    fireEvent.click(screen.getByText('Strobe'));
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.click(container.firstElementChild as HTMLElement);
    expect(onClose).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(document, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it('surfaces a load error', async () => {
    apiMock.track.mockRejectedValueOnce(new Error('Track not found'));
    render(<TrackInfoModal trackId="track-1" onClose={vi.fn()} onSaved={vi.fn()} />);
    expect(await screen.findByText('Track not found')).toBeInTheDocument();
  });

  it('warns before a rename that would detach the track from a merged artist', async () => {
    apiMock.artists.mockResolvedValue([{ id: 'artist-1', name: 'deadmau5' }]);
    apiMock.getArtistMergeInfo.mockResolvedValue({
      merged: true,
      members: [{ id: 'm1', original_name: 'Deadmau5', album_count: 1, track_count: 1 }],
    });
    render(<TrackInfoModal trackId="track-1" onClose={vi.fn()} onSaved={vi.fn()} />);
    await screen.findByDisplayValue('Strobe');
    await waitFor(() => expect(apiMock.getArtistMergeInfo).toHaveBeenCalledWith('artist-1'));

    // Unchanged from the track's current (merged) artist — no warning yet.
    expect(screen.queryByText(/renaming this track.s artist will detach it/)).not.toBeInTheDocument();

    fireEvent.change(screen.getByDisplayValue('deadmau5'), { target: { value: 'Joel Zimmerman' } });
    expect(await screen.findByText(/renaming this track.s artist will detach it/)).toBeInTheDocument();

    // Restoring the original name (even with different casing/whitespace) clears it.
    // (Composer also happens to read "Joel Zimmerman" in this fixture — the
    // Artist field renders first, per the Title/Artist row.)
    fireEvent.change(screen.getAllByDisplayValue('Joel Zimmerman')[0], { target: { value: ' DEADMAU5 ' } });
    await waitFor(() => expect(
      screen.queryByText(/renaming this track.s artist will detach it/),
    ).not.toBeInTheDocument());
  });

  it('does not warn when the track artist is not a merge master', async () => {
    apiMock.artists.mockResolvedValue([{ id: 'artist-1', name: 'deadmau5' }]);
    apiMock.getArtistMergeInfo.mockResolvedValue({ merged: false, members: [] });
    render(<TrackInfoModal trackId="track-1" onClose={vi.fn()} onSaved={vi.fn()} />);
    await screen.findByDisplayValue('Strobe');

    fireEvent.change(screen.getByDisplayValue('deadmau5'), { target: { value: 'Joel Zimmerman' } });
    expect(screen.queryByText(/renaming this track.s artist will detach it/)).not.toBeInTheDocument();
  });
});

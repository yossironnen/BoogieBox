import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import MetadataEditModal from './MetadataEditModal';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    genres: vi.fn(),
    uploadAlbumArtwork: vi.fn(),
    uploadArtistArtwork: vi.fn(),
    updateAlbumMetadata: vi.fn(),
    updateArtistMetadata: vi.fn(),
  },
}));

vi.mock('../api', () => ({ api: apiMock }));

const album = {
  id: 'album-1',
  title: 'Old title',
  album_artist: 'Old artist',
  year: 1999,
  genre: 'Rock',
  releaseType: 'album',
  description: 'Old notes',
  metadata_locked: true,
} as any;

const artist = {
  id: 'artist-1',
  name: 'Artist name',
  description: '',
  metadata_locked: false,
} as any;

describe('MetadataEditModal', () => {
  beforeEach(() => {
    Object.values(apiMock).forEach(mock => mock.mockReset());
    apiMock.genres.mockResolvedValue([{ genre: 'Rock' }, { genre: '' }, { genre: 'Jazz' }]);
    apiMock.uploadAlbumArtwork.mockResolvedValue({});
    apiMock.uploadArtistArtwork.mockResolvedValue({});
    apiMock.updateAlbumMetadata.mockResolvedValue({});
    apiMock.updateArtistMetadata.mockResolvedValue({});
  });

  it('edits album fields, uploads artwork, and saves normalized metadata', async () => {
    const onSaved = vi.fn();
    const { container } = render(
      <MetadataEditModal
        mode="album"
        entityId="album-1"
        initialData={album}
        onClose={vi.fn()}
        onSaved={onSaved}
      />,
    );

    expect(screen.getByText(/Custom metadata/)).toBeInTheDocument();
    await waitFor(() => expect(apiMock.genres).toHaveBeenCalled());
    expect(container.querySelectorAll('datalist option')).toHaveLength(2);

    fireEvent.change(screen.getByDisplayValue('Old title'), { target: { value: ' New title ' } });
    fireEvent.change(screen.getByDisplayValue('Old artist'), { target: { value: ' New artist ' } });
    fireEvent.change(screen.getByDisplayValue('1999'), { target: { value: '2024' } });
    fireEvent.change(screen.getByDisplayValue('Rock'), { target: { value: ' Electronic ' } });
    fireEvent.change(screen.getByDisplayValue('Old notes'), { target: { value: ' Updated notes ' } });
    fireEvent.change(screen.getByDisplayValue('Album'), { target: { value: 'compilation' } });

    const fileInput = container.querySelector<HTMLInputElement>('input[type="file"]')!;
    fireEvent.change(fileInput, {
      target: { files: [new File(['cover'], 'cover.png', { type: 'image/png' })] },
    });
    expect(await screen.findByAltText('preview')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(apiMock.uploadAlbumArtwork).toHaveBeenCalledWith(
      'album-1',
      expect.any(String),
      'image/png',
    ));
    expect(apiMock.updateAlbumMetadata).toHaveBeenCalledWith('album-1', {
      title: 'New title',
      album_artist: 'New artist',
      year: 2024,
      genre: 'Electronic',
      description: 'Updated notes',
      releaseType: 'compilation',
    });
    expect(onSaved).toHaveBeenCalled();
  });

  it('removes an artwork selection and handles album save failures', async () => {
    apiMock.updateAlbumMetadata.mockRejectedValueOnce(new Error('save rejected'));
    const { container } = render(
      <MetadataEditModal
        mode="album"
        entityId="album-1"
        initialData={album}
        onClose={vi.fn()}
        onSaved={vi.fn()}
      />,
    );
    const current = screen.getByAltText('current');
    fireEvent.error(current);
    expect(current).toHaveStyle({ display: 'none' });

    const fileInput = container.querySelector<HTMLInputElement>('input[type="file"]')!;
    fireEvent.change(fileInput, {
      target: { files: [new File(['cover'], 'cover.jpg', { type: '' })] },
    });
    await screen.findByAltText('preview');
    fireEvent.click(screen.getByRole('button', { name: 'Remove selection' }));
    expect(screen.queryByAltText('preview')).not.toBeInTheDocument();

    fireEvent.change(screen.getByDisplayValue('1999'), { target: { value: '' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('save rejected')).toBeInTheDocument();
    expect(apiMock.updateAlbumMetadata).toHaveBeenCalledWith(
      'album-1',
      expect.objectContaining({ year: undefined }),
    );
  });

  it('validates and saves artist metadata with optional artwork', async () => {
    const onSaved = vi.fn();
    const { container } = render(
      <MetadataEditModal
        mode="artist"
        entityId="artist-1"
        initialData={artist}
        onClose={vi.fn()}
        onSaved={onSaved}
      />,
    );
    const nameInput = screen.getByDisplayValue('Artist name');
    fireEvent.change(nameInput, { target: { value: '   ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(screen.getByText('Name is required')).toBeInTheDocument();
    expect(apiMock.updateArtistMetadata).not.toHaveBeenCalled();

    fireEvent.change(nameInput, { target: { value: ' Renamed artist ' } });
    fireEvent.change(screen.getByPlaceholderText('Optional notes or description…'), {
      target: { value: ' Artist notes ' },
    });
    const fileInput = container.querySelector<HTMLInputElement>('input[type="file"]')!;
    fireEvent.change(fileInput, {
      target: { files: [new File(['photo'], 'photo.webp', { type: 'image/webp' })] },
    });
    await screen.findByAltText('preview');
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(apiMock.uploadArtistArtwork).toHaveBeenCalledWith(
      'artist-1',
      expect.any(String),
      'image/webp',
    ));
    expect(apiMock.updateArtistMetadata).toHaveBeenCalledWith('artist-1', {
      name: 'Renamed artist',
      description: 'Artist notes',
    });
    expect(onSaved).toHaveBeenCalled();
  });

  it('closes from the header, footer, and backdrop without treating inner clicks as backdrop clicks', () => {
    const onClose = vi.fn();
    const { container } = render(
      <MetadataEditModal
        mode="artist"
        entityId="artist-1"
        initialData={artist}
        onClose={onClose}
        onSaved={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByText('Edit Artist'));
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    fireEvent.click(screen.getByRole('button', { name: '✕' }));
    fireEvent.click(container.firstElementChild as HTMLElement);
    expect(onClose).toHaveBeenCalledTimes(3);
  });
});

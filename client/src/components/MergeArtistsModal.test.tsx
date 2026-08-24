import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import MergeArtistsModal from './MergeArtistsModal';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    mergeArtists: vi.fn(),
  },
}));

vi.mock('../api', () => ({ api: apiMock }));

const artists = [
  { id: 'a1', name: 'Madonna', album_count: 11, track_count: 142 } as any,
  { id: 'a2', name: 'Madonna Ciccone', album_count: 2, track_count: 19 } as any,
];

describe('MergeArtistsModal', () => {
  beforeEach(() => {
    apiMock.mergeArtists.mockReset();
  });

  it('defaults to the first artist as master and shows the result preview', () => {
    render(<MergeArtistsModal artists={artists} onClose={vi.fn()} onMerged={vi.fn()} />);
    expect(screen.getByText('Merge 2 artists into one')).toBeInTheDocument();
    expect(screen.getByText(/13 albums, 161 tracks/)).toBeInTheDocument();
  });

  it('merges using the selected existing artist as master', async () => {
    apiMock.mergeArtists.mockResolvedValue({ id: 'a1', name: 'Madonna Ciccone' });
    const onMerged = vi.fn();
    render(<MergeArtistsModal artists={artists} onClose={vi.fn()} onMerged={onMerged} />);

    fireEvent.click(screen.getAllByRole('radio')[1]);
    fireEvent.click(screen.getByRole('button', { name: 'Merge Artists' }));

    await waitFor(() => expect(apiMock.mergeArtists).toHaveBeenCalledWith(
      ['a1', 'a2'],
      'Madonna Ciccone',
      'a2',
    ));
    expect(onMerged).toHaveBeenCalledWith({ id: 'a1', name: 'Madonna Ciccone' });
  });

  it('merges using a custom typed name (no master_artist_id)', async () => {
    apiMock.mergeArtists.mockResolvedValue({ id: 'new-1', name: 'The Material Girl' });
    const onMerged = vi.fn();
    render(<MergeArtistsModal artists={artists} onClose={vi.fn()} onMerged={onMerged} />);

    fireEvent.change(screen.getByPlaceholderText('Type a new artist name…'), {
      target: { value: 'The Material Girl' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Merge Artists' }));

    await waitFor(() => expect(apiMock.mergeArtists).toHaveBeenCalledWith(
      ['a1', 'a2'],
      'The Material Girl',
      undefined,
    ));
    expect(onMerged).toHaveBeenCalled();
  });

  it('disables Merge when the custom-name field is chosen but empty', () => {
    render(<MergeArtistsModal artists={artists} onClose={vi.fn()} onMerged={vi.fn()} />);
    fireEvent.click(screen.getAllByRole('radio')[2]); // "Use a custom name"
    expect(screen.getByRole('button', { name: 'Merge Artists' })).toBeDisabled();
  });

  it('shows the server error message on failure', async () => {
    apiMock.mergeArtists.mockRejectedValue(new Error('Various Artists cannot be merged'));
    render(<MergeArtistsModal artists={artists} onClose={vi.fn()} onMerged={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Merge Artists' }));
    expect(await screen.findByText('Various Artists cannot be merged')).toBeInTheDocument();
  });

  it('falls back to a generic message when the failure has none', async () => {
    apiMock.mergeArtists.mockRejectedValue({});
    render(<MergeArtistsModal artists={artists} onClose={vi.fn()} onMerged={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Merge Artists' }));
    expect(await screen.findByText('Merge failed')).toBeInTheDocument();
  });

  it('focusing the custom-name field selects the custom-name option', () => {
    render(<MergeArtistsModal artists={artists} onClose={vi.fn()} onMerged={vi.fn()} />);
    fireEvent.focus(screen.getByPlaceholderText('Type a new artist name…'));
    expect(screen.getAllByRole('radio')[2]).toBeChecked();
  });

  it('cancel calls onClose without merging', () => {
    const onClose = vi.fn();
    render(<MergeArtistsModal artists={artists} onClose={onClose} onMerged={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onClose).toHaveBeenCalled();
    expect(apiMock.mergeArtists).not.toHaveBeenCalled();
  });

  it('clicking the backdrop closes the modal, clicking inside does not', () => {
    const onClose = vi.fn();
    const { container } = render(<MergeArtistsModal artists={artists} onClose={onClose} onMerged={vi.fn()} />);
    fireEvent.click(screen.getByText('Merge 2 artists into one'));
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.click(container.firstElementChild as HTMLElement);
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

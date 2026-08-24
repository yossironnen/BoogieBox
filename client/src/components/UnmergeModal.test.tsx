import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import UnmergeModal from './UnmergeModal';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    unmergeArtist: vi.fn(),
  },
}));

vi.mock('../api', () => ({ api: apiMock }));

const members = [
  { id: 'm1', original_name: 'Madonna Ciccone', album_count: 2, track_count: 19 } as any,
  { id: 'm2', original_name: 'M.D.N.A. (Madonna)', album_count: 1, track_count: 12 } as any,
];

describe('UnmergeModal', () => {
  beforeEach(() => {
    apiMock.unmergeArtist.mockReset();
  });

  it('starts with every member checked and unmerges all by default', async () => {
    apiMock.unmergeArtist.mockResolvedValue({ master: null, new_artist_ids: ['n1', 'n2'] });
    const onUnmerged = vi.fn();
    render(
      <UnmergeModal artistId="a1" artistName="Madonna" members={members} onClose={vi.fn()} onUnmerged={onUnmerged} />,
    );

    expect(screen.getByRole('button', { name: 'Unmerge Selected (2)' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Unmerge Selected (2)' }));

    await waitFor(() => expect(apiMock.unmergeArtist).toHaveBeenCalledWith('a1', ['m1', 'm2']));
    expect(onUnmerged).toHaveBeenCalledWith({ master: null, new_artist_ids: ['n1', 'n2'] });
  });

  it('unchecking a member excludes it from the unmerge call', async () => {
    apiMock.unmergeArtist.mockResolvedValue({ master: { id: 'a1' }, new_artist_ids: ['n1'] });
    render(
      <UnmergeModal artistId="a1" artistName="Madonna" members={members} onClose={vi.fn()} onUnmerged={vi.fn()} />,
    );

    fireEvent.click(screen.getByText('M.D.N.A. (Madonna)').closest('label')!.querySelector('input')!);
    expect(screen.getByRole('button', { name: 'Unmerge Selected (1)' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Unmerge Selected (1)' }));

    await waitFor(() => expect(apiMock.unmergeArtist).toHaveBeenCalledWith('a1', ['m1']));
  });

  it('disables the button when every member is unchecked', () => {
    render(
      <UnmergeModal artistId="a1" artistName="Madonna" members={members} onClose={vi.fn()} onUnmerged={vi.fn()} />,
    );
    members.forEach((m) => {
      fireEvent.click(screen.getByText(m.original_name).closest('label')!.querySelector('input')!);
    });
    expect(screen.getByRole('button', { name: 'Unmerge Selected (0)' })).toBeDisabled();
  });

  it('shows the server error message on failure', async () => {
    apiMock.unmergeArtist.mockRejectedValue(new Error('Artist is not a merged identity'));
    render(
      <UnmergeModal artistId="a1" artistName="Madonna" members={members} onClose={vi.fn()} onUnmerged={vi.fn()} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Unmerge Selected (2)' }));
    expect(await screen.findByText('Artist is not a merged identity')).toBeInTheDocument();
  });

  it('falls back to a generic message when the failure has none', async () => {
    apiMock.unmergeArtist.mockRejectedValue({});
    render(
      <UnmergeModal artistId="a1" artistName="Madonna" members={members} onClose={vi.fn()} onUnmerged={vi.fn()} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Unmerge Selected (2)' }));
    expect(await screen.findByText('Unmerge failed')).toBeInTheDocument();
  });

  it('re-checking a previously unchecked member restores it to the selection', async () => {
    apiMock.unmergeArtist.mockResolvedValue({ master: null, new_artist_ids: ['n1', 'n2'] });
    render(
      <UnmergeModal artistId="a1" artistName="Madonna" members={members} onClose={vi.fn()} onUnmerged={vi.fn()} />,
    );
    const checkbox = screen.getByText('M.D.N.A. (Madonna)').closest('label')!.querySelector('input')!;
    fireEvent.click(checkbox); // uncheck
    fireEvent.click(checkbox); // re-check
    expect(screen.getByRole('button', { name: 'Unmerge Selected (2)' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Unmerge Selected (2)' }));
    await waitFor(() => expect(apiMock.unmergeArtist).toHaveBeenCalledWith('a1', ['m1', 'm2']));
  });

  it('clicking the backdrop closes the modal, clicking inside does not', () => {
    const onClose = vi.fn();
    const { container } = render(
      <UnmergeModal artistId="a1" artistName="Madonna" members={members} onClose={onClose} onUnmerged={vi.fn()} />,
    );
    fireEvent.click(screen.getByText('Unmerge Madonna'));
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.click(container.firstElementChild as HTMLElement);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('cancel calls onClose without unmerging', () => {
    const onClose = vi.fn();
    render(
      <UnmergeModal artistId="a1" artistName="Madonna" members={members} onClose={onClose} onUnmerged={vi.fn()} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onClose).toHaveBeenCalled();
    expect(apiMock.unmergeArtist).not.toHaveBeenCalled();
  });
});

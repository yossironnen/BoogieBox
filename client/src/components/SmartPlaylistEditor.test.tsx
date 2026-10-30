/**
 * Tests the smart playlist rule editor: creating, previewing, marking wrong values, and edit conflicts.
 */

// @vitest-environment jsdom

import React from 'react';
import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { apiMock, ApiError } = vi.hoisted(() => {
  class ApiError extends Error {
    constructor(message: string, readonly status: number, readonly code: string | null, readonly body: unknown) {
      super(message);
    }
  }
  return {
    ApiError,
    apiMock: {
      playlists: {
        createSmart: vi.fn(),
        smartPreview: vi.fn(),
        smartDefinition: vi.fn(),
        updateSmartDefinition: vi.fn(),
        update: vi.fn(),
      },
      libraries: { list: vi.fn() },
      genres: vi.fn(),
      search: vi.fn(),
    },
  };
});

vi.mock('../api', () => ({ api: apiMock, SmartPlaylistApiError: ApiError }));

import SmartPlaylistEditor from './SmartPlaylistEditor';

const preview = {
  trackCount: 2,
  totalDuration: 600,
  tracks: [
    { id: 't1', title: 'Teardrop', artist: 'Massive Attack', duration: 329, file_name: 'a.mp3' },
    { id: 't2', title: 'Glory Box', artist: 'Portishead', duration: 301, file_name: 'b.mp3' },
  ],
  missingLibraryIds: [],
};

const stored = {
  definition: {
    schemaVersion: 1,
    root: { mode: 'all', children: [{ field: 'genre', operator: 'is', value: 'Trip Hop' }] },
    sort: [{ field: 'title', direction: 'asc' }],
    limit: { maxTracks: 100 },
  },
  schemaVersion: 1,
  editRevision: 3,
  membershipRevision: 1,
  evaluatedAt: null,
  supported: true,
  missingLibraryIds: [],
};

const smartPlaylist = {
  id: 'p1', name: 'Late night', description: null, created_at: null, updated_at: null,
  track_count: 2, total_duration: 600, kind: 'smart' as const,
};

describe('SmartPlaylistEditor', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
    HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
    apiMock.libraries.list.mockResolvedValue([{ id: 'lib1', name: 'Music', path: '/m', added_at: '' }]);
    apiMock.genres.mockResolvedValue([{ genre: 'Trip Hop', track_count: 2 }]);
    apiMock.search.mockResolvedValue({ artists: [], albums: [], tracks: [] });
    apiMock.playlists.smartPreview.mockResolvedValue(preview);
  });

  afterEach(() => cleanup());

  it('previews valid rules and creates the playlist', async () => {
    const onSaved = vi.fn();
    render(<SmartPlaylistEditor existingNames={['Road Trip']} onClose={vi.fn()} onSaved={onSaved} />);
    const dialog = screen.getByRole('dialog', { name: 'New smart playlist' });
    fireEvent.change(within(dialog).getByLabelText('Playlist name'), { target: { value: 'Trip hop' } });
    fireEvent.change(within(dialog).getByRole('combobox', { name: 'Artist name' }), { target: { value: 'Portishead' } });

    expect(await within(dialog).findByRole('list', { name: 'First tracks' })).toHaveTextContent('Glory Box');
    expect(apiMock.playlists.smartPreview).toHaveBeenCalledWith(
      expect.objectContaining({ root: { mode: 'all', children: [{ field: 'artist', operator: 'is', value: 'Portishead' }] } }),
      expect.any(AbortSignal),
    );
    expect(within(dialog).getByText('Tracks').previousSibling).toHaveTextContent('2');

    apiMock.playlists.createSmart.mockResolvedValue({ ...smartPlaylist, name: 'Trip hop' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Create playlist' }));
    await waitFor(() => expect(onSaved).toHaveBeenCalledWith(expect.objectContaining({ name: 'Trip hop' })));
    expect(apiMock.playlists.createSmart).toHaveBeenCalledWith('Trip hop', expect.objectContaining({ schemaVersion: 1 }));
  });

  it('marks wrong values instead of saving', async () => {
    render(<SmartPlaylistEditor existingNames={[]} onClose={vi.fn()} onSaved={vi.fn()} />);
    const dialog = screen.getByRole('dialog', { name: 'New smart playlist' });
    fireEvent.change(within(dialog).getByLabelText('Playlist name'), { target: { value: 'Empty' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Create playlist' }));
    expect(await within(dialog).findByText('Enter an artist name.')).toBeVisible();
    expect(within(dialog).getByText('Fix the marked rules to save.')).toBeVisible();
    expect(apiMock.playlists.createSmart).not.toHaveBeenCalled();
    expect(within(dialog).getByRole('button', { name: 'Remove rule' })).toBeDisabled();
  });

  it('refuses a name already in use', async () => {
    render(<SmartPlaylistEditor existingNames={['Road Trip']} onClose={vi.fn()} onSaved={vi.fn()} />);
    const dialog = screen.getByRole('dialog', { name: 'New smart playlist' });
    fireEvent.change(within(dialog).getByLabelText('Playlist name'), { target: { value: ' road  trip ' } });
    fireEvent.change(within(dialog).getByRole('combobox', { name: 'Artist name' }), { target: { value: 'Air' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Create playlist' }));
    expect(await within(dialog).findByText('A playlist with this name already exists')).toBeVisible();
    expect(apiMock.playlists.createSmart).not.toHaveBeenCalled();
  });

  it('adds groups and shows the empty preview', async () => {
    apiMock.playlists.smartPreview.mockResolvedValue({ ...preview, trackCount: 0, totalDuration: 0, tracks: [] });
    render(<SmartPlaylistEditor existingNames={[]} onClose={vi.fn()} onSaved={vi.fn()} />);
    const dialog = screen.getByRole('dialog', { name: 'New smart playlist' });
    fireEvent.click(within(dialog).getAllByRole('button', { name: 'Add group' })[0]);
    const group = within(dialog).getByRole('group', { name: 'Rule group' });
    expect(within(group).getByRole('combobox', { name: 'Match all or any rule in this group' })).toHaveValue('any');
    fireEvent.change(within(group).getByRole('combobox', { name: 'Field' }), { target: { value: 'favourite' } });
    fireEvent.change(within(dialog).getAllByRole('combobox', { name: 'Artist name' })[0], { target: { value: 'Air' } });
    expect(await within(dialog).findByText('No tracks match these rules.')).toBeVisible();
    expect(within(dialog).getByText('You can still save it. Tracks appear when they match.')).toBeVisible();
    fireEvent.click(within(group).getByRole('button', { name: /Remove group/ }));
    expect(within(dialog).queryByRole('group', { name: 'Rule group' })).toBeNull();
  });

  it('loads saved rules and resolves an edit conflict', async () => {
    apiMock.playlists.smartDefinition.mockResolvedValue(stored);
    apiMock.playlists.updateSmartDefinition
      .mockRejectedValueOnce(new ApiError('changed', 409, 'edit_conflict', { editRevision: 5 }))
      .mockResolvedValueOnce({ ...stored, editRevision: 6 });
    const onSaved = vi.fn();
    render(<SmartPlaylistEditor playlist={smartPlaylist} existingNames={['Late night']} onClose={vi.fn()} onSaved={onSaved} />);
    const dialog = screen.getByRole('dialog', { name: 'Edit rules' });
    expect(await within(dialog).findByRole('combobox', { name: 'Genre' })).toHaveValue('Trip Hop');

    fireEvent.click(within(dialog).getByRole('button', { name: 'Save rules' }));
    expect(await within(dialog).findByText('These rules were changed on another device while you were editing.')).toBeVisible();
    expect(apiMock.playlists.updateSmartDefinition).toHaveBeenLastCalledWith('p1', expect.any(Object), 3);

    fireEvent.click(within(dialog).getByRole('button', { name: 'Save mine anyway' }));
    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    expect(apiMock.playlists.updateSmartDefinition).toHaveBeenLastCalledWith('p1', expect.any(Object), 5);
    expect(apiMock.playlists.update).not.toHaveBeenCalled();
  });

  it('keeps rules from a newer version read-only', async () => {
    apiMock.playlists.smartDefinition.mockResolvedValue({ ...stored, schemaVersion: 2, supported: false });
    render(<SmartPlaylistEditor playlist={smartPlaylist} existingNames={[]} onClose={vi.fn()} onSaved={vi.fn()} />);
    const dialog = screen.getByRole('dialog', { name: 'Edit rules' });
    expect(await within(dialog).findByText("These rules were made by a newer version of BoogieBox and can’t be edited here.")).toBeVisible();
    expect(within(dialog).queryByRole('button', { name: 'Save rules' })).toBeNull();
  });

  it('says so and logs it when saved rules fail to load', async () => {
    const failure = new ApiError('Internal server error', 500, null, null);
    apiMock.playlists.smartDefinition.mockRejectedValue(failure);
    const logged = vi.spyOn(console, 'error').mockImplementation(() => {});
    render(<SmartPlaylistEditor playlist={smartPlaylist} existingNames={[]} onClose={vi.fn()} onSaved={vi.fn()} />);
    const dialog = screen.getByRole('dialog', { name: 'Edit rules' });
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('Could not load saved rules.');
    expect(logged).toHaveBeenCalledWith('Could not load saved smart playlist rules', failure);
    expect(within(dialog).getByRole('button', { name: 'Save rules' })).toHaveAttribute('aria-disabled', 'true');
    logged.mockRestore();
  });

  it('offers a retry when the preview fails', async () => {
    apiMock.playlists.smartPreview.mockRejectedValueOnce(new ApiError('boom', 500, null, null));
    render(<SmartPlaylistEditor existingNames={[]} onClose={vi.fn()} onSaved={vi.fn()} />);
    const dialog = screen.getByRole('dialog', { name: 'New smart playlist' });
    fireEvent.change(within(dialog).getByRole('combobox', { name: 'Artist name' }), { target: { value: 'Air' } });
    expect(await within(dialog).findByText('Couldn’t update the preview.')).toBeVisible();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Try again' }));
    expect(await within(dialog).findByRole('list', { name: 'First tracks' })).toBeVisible();
  });
});

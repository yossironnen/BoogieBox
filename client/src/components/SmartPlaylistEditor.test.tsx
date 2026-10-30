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
import { SMART_IDEAS, checkDraft, definitionToDraft, genrePreset } from '../smartPlaylistRules';

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

  it('on phones keeps the preview behind "Show tracks" and stacks each rule', async () => {
    const onSaved = vi.fn();
    render(<SmartPlaylistEditor variant="mobile" existingNames={[]} onClose={vi.fn()} onSaved={onSaved} />);
    const dialog = screen.getByRole('dialog', { name: 'New smart playlist' });
    expect(within(dialog).queryByRole('button', { name: 'Cancel' })).toBeNull();
    expect(within(dialog).getByLabelText('Playlist name')).not.toHaveFocus();
    expect(within(dialog).getByRole('combobox', { name: 'Field' }).style.minHeight).toBe('44px');

    fireEvent.change(within(dialog).getByLabelText('Playlist name'), { target: { value: 'Trip hop' } });
    fireEvent.change(within(dialog).getByRole('combobox', { name: 'Artist name' }), { target: { value: 'Portishead' } });
    const summary = await within(dialog).findByRole('button', { name: /2 tracks\s*· 10m\s*Show tracks/ });
    expect(summary).toHaveAttribute('aria-expanded', 'false');
    expect(within(dialog).queryByRole('list', { name: 'First tracks' })).toBeNull();

    fireEvent.click(summary);
    expect(summary).toHaveAttribute('aria-expanded', 'true');
    expect(within(dialog).getByRole('list', { name: 'First tracks' })).toHaveTextContent('Glory Box');

    fireEvent.click(within(dialog).getAllByRole('button', { name: 'Add group' })[0]);
    expect(within(within(dialog).getByRole('group', { name: 'Rule group' })).getByRole('button', { name: 'Remove group' })).toBeVisible();

    apiMock.playlists.createSmart.mockResolvedValue({ ...smartPlaylist, name: 'Trip hop' });
    fireEvent.change(within(dialog).getAllByRole('combobox', { name: 'Artist name' })[1], { target: { value: 'Air' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Create playlist' }));
    await waitFor(() => expect(onSaved).toHaveBeenCalled());
  });

  it('builds number, range, rating, date, day and library rules', async () => {
    render(<SmartPlaylistEditor existingNames={[]} onClose={vi.fn()} onSaved={vi.fn()} />);
    const dialog = screen.getByRole('dialog', { name: 'New smart playlist' });
    await waitFor(() => expect(apiMock.libraries.list).toHaveBeenCalled());
    const rows: [string, string][] = [
      ['year', 'between'], ['bpm', 'gte'], ['rating', 'gte'], ['rating', 'between'],
      ['added_at', 'before'], ['last_played_at', 'not_in_last_days'], ['library_id', 'in'], ['play_count', 'is'],
    ];
    rows.forEach(([field, operator], i) => {
      if (i > 0) fireEvent.click(within(dialog).getAllByRole('button', { name: 'Add rule' })[0]);
      fireEvent.change(within(dialog).getAllByRole('combobox', { name: 'Field' })[i], { target: { value: field } });
      fireEvent.change(within(dialog).getAllByRole('combobox', { name: 'Condition' })[i], { target: { value: operator } });
    });
    fireEvent.change(within(dialog).getByLabelText('From year'), { target: { value: '1990' } });
    fireEvent.change(within(dialog).getByLabelText('To year'), { target: { value: '1999' } });
    fireEvent.change(within(dialog).getByLabelText('BPM'), { target: { value: '120' } });
    fireEvent.keyDown(within(dialog).getByRole('slider', { name: 'Rating' }), { key: 'ArrowRight' });
    fireEvent.keyDown(within(dialog).getByRole('slider', { name: 'From rating' }), { key: 'ArrowRight' });
    fireEvent.keyDown(within(dialog).getByRole('slider', { name: 'To rating' }), { key: 'ArrowRight' });
    fireEvent.keyDown(within(dialog).getByRole('slider', { name: 'To rating' }), { key: 'ArrowRight' });
    fireEvent.change(within(dialog).getByLabelText('Date'), { target: { value: '2020-01-01' } });
    fireEvent.change(within(dialog).getByLabelText('Days'), { target: { value: '30' } });
    expect(within(dialog).getByText("Includes tracks you've never played.")).toBeVisible();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Music' }));
    expect(within(dialog).getByRole('button', { name: 'Music' })).toHaveAttribute('aria-pressed', 'true');
    fireEvent.change(within(dialog).getByLabelText('Plays'), { target: { value: '3' } });

    await waitFor(() => expect(apiMock.playlists.smartPreview).toHaveBeenCalled());
    const definition = apiMock.playlists.smartPreview.mock.lastCall![0];
    expect(definition.root.children.map((c: { field: string; operator: string }) => [c.field, c.operator])).toEqual(rows);
    expect(definition.root.children[0].value).toEqual([1990, 1999]);
    expect(definition.root.children[1].value).toBe(120);
    expect(definition.root.children[6].value).toEqual(['lib1']);
    expect(definition.root.children[7].value).toBe(3);

    fireEvent.click(within(dialog).getAllByRole('button', { name: 'Remove rule' })[7]);
    expect(within(dialog).queryByLabelText('Plays')).toBeNull();
  });

  it('flags a removed library and lets it be taken out', async () => {
    apiMock.playlists.smartDefinition.mockResolvedValue({
      ...stored,
      definition: { ...stored.definition, root: { mode: 'all', children: [{ field: 'library_id', operator: 'in', value: ['lib1', 'gone'] }] } },
      missingLibraryIds: ['gone'],
    });
    render(<SmartPlaylistEditor playlist={smartPlaylist} existingNames={[]} onClose={vi.fn()} onSaved={vi.fn()} />);
    const dialog = screen.getByRole('dialog', { name: 'Edit rules' });
    const removed = await within(dialog).findByRole('button', { name: 'Removed library' });
    expect(within(dialog).getByText('This library was removed. Take it out of the rule or choose another.')).toBeVisible();
    fireEvent.click(removed);
    expect(within(dialog).queryByRole('button', { name: 'Removed library' })).toBeNull();
    expect(within(dialog).queryByText('This library was removed. Take it out of the rule or choose another.')).toBeNull();
  });

  it('sets the order, a second order and the limits', async () => {
    render(<SmartPlaylistEditor existingNames={[]} onClose={vi.fn()} onSaved={vi.fn()} />);
    const dialog = screen.getByRole('dialog', { name: 'New smart playlist' });
    fireEvent.change(within(dialog).getByRole('combobox', { name: 'Artist name' }), { target: { value: 'Air' } });
    fireEvent.change(within(dialog).getByRole('combobox', { name: 'Match all or any rule' }), { target: { value: 'any' } });
    fireEvent.change(within(dialog).getByLabelText('Order by'), { target: { value: 'year' } });
    fireEvent.change(within(dialog).getByRole('combobox', { name: 'Direction' }), { target: { value: 'desc' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Add a second order' }));
    fireEvent.change(within(dialog).getByRole('combobox', { name: 'Then by' }), { target: { value: 'rating' } });
    fireEvent.change(within(dialog).getByRole('combobox', { name: 'Then by direction' }), { target: { value: 'desc' } });
    fireEvent.change(within(dialog).getByLabelText('Up to'), { target: { value: '50' } });
    expect(within(dialog).getByLabelText('Minutes')).toBeDisabled();
    fireEvent.click(within(dialog).getByRole('checkbox', { name: 'Limit the total length to' }));
    fireEvent.change(within(dialog).getByLabelText('Minutes'), { target: { value: '90' } });

    await waitFor(() => expect(apiMock.playlists.smartPreview.mock.lastCall?.[0]).toMatchObject({
      root: { mode: 'any' },
      sort: [{ field: 'year', direction: 'desc' }, { field: 'rating', direction: 'desc' }],
      limit: { maxTracks: 50, maxDurationSeconds: 5400 },
    }));

    // Ordering by the second field drops the duplicate second order.
    fireEvent.change(within(dialog).getByLabelText('Order by'), { target: { value: 'rating' } });
    expect(within(dialog).queryByRole('combobox', { name: 'Then by' })).toBeNull();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Add a second order' }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Remove second order' }));
    expect(within(dialog).queryByRole('combobox', { name: 'Then by' })).toBeNull();

    fireEvent.change(within(dialog).getByLabelText('Up to'), { target: { value: '5000' } });
    fireEvent.change(within(dialog).getByLabelText('Minutes'), { target: { value: '0' } });
    expect(within(dialog).getByText('Enter 1 to 1,000 tracks.')).toBeVisible();
    expect(within(dialog).getByText('Enter 1 to 1,440 minutes.')).toBeVisible();
  });

  it('on phones shows the minutes only with the length limit', async () => {
    render(<SmartPlaylistEditor variant="mobile" existingNames={[]} onClose={vi.fn()} onSaved={vi.fn()} />);
    const dialog = screen.getByRole('dialog', { name: 'New smart playlist' });
    fireEvent.change(within(dialog).getByRole('combobox', { name: 'Artist name' }), { target: { value: 'Air' } });
    expect(within(dialog).queryByLabelText('Minutes')).toBeNull();
    fireEvent.click(within(dialog).getByRole('checkbox', { name: 'Limit the total length to' }));
    fireEvent.change(within(dialog).getByLabelText('Minutes'), { target: { value: '45' } });
    fireEvent.change(within(dialog).getByLabelText('Up to'), { target: { value: '20' } });
    fireEvent.change(within(dialog).getByLabelText('Order by'), { target: { value: 'artist' } });
    fireEvent.change(within(dialog).getByRole('combobox', { name: 'Direction' }), { target: { value: 'desc' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Add a second order' }));
    fireEvent.change(within(dialog).getByRole('combobox', { name: 'Then by' }), { target: { value: 'year' } });
    await waitFor(() => expect(apiMock.playlists.smartPreview.mock.lastCall?.[0]).toMatchObject({
      sort: [{ field: 'artist', direction: 'desc' }, { field: 'year', direction: 'asc' }],
      limit: { maxTracks: 20, maxDurationSeconds: 2700 },
    }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Remove second order' }));
    expect(within(dialog).queryByRole('combobox', { name: 'Then by' })).toBeNull();
    fireEvent.change(within(dialog).getByLabelText('Minutes'), { target: { value: '9999' } });
    expect(within(dialog).getByText('Enter 1 to 1,440 minutes.')).toBeVisible();
  });

  it('suggests artist and album names as you type', async () => {
    apiMock.search.mockResolvedValue({ artists: [{ name: 'Portishead' }, { name: 'Portishead' }], albums: [{ title: 'Dummy' }], tracks: [] });
    render(<SmartPlaylistEditor existingNames={[]} onClose={vi.fn()} onSaved={vi.fn()} />);
    const dialog = screen.getByRole('dialog', { name: 'New smart playlist' });
    const artist = within(dialog).getByRole('combobox', { name: 'Artist name' });
    fireEvent.change(artist, { target: { value: 'Por' } });
    await waitFor(() => expect(dialog.querySelectorAll(`#${artist.getAttribute('list')} option`)).toHaveLength(1));
    expect(apiMock.search).toHaveBeenLastCalledWith(expect.objectContaining({ q: 'Por', include_artists: true }));

    fireEvent.change(within(dialog).getByRole('combobox', { name: 'Field' }), { target: { value: 'album' } });
    const album = within(dialog).getByRole('combobox', { name: 'Album name' });
    apiMock.search.mockRejectedValueOnce(new Error('offline'));
    fireEvent.change(album, { target: { value: 'Du' } });
    await waitFor(() => expect(apiMock.search).toHaveBeenLastCalledWith(expect.objectContaining({ q: 'Du', include_albums: true })));
    fireEvent.change(album, { target: { value: 'Dum' } });
    await waitFor(() => expect(dialog.querySelector(`#${album.getAttribute('list')} option`)).toHaveAttribute('value', 'Dummy'));
  });

  it('waits and asks again when the server is busy', async () => {
    apiMock.playlists.smartPreview.mockRejectedValueOnce(new ApiError('busy', 429, 'busy', null));
    render(<SmartPlaylistEditor existingNames={[]} onClose={vi.fn()} onSaved={vi.fn()} />);
    const dialog = screen.getByRole('dialog', { name: 'New smart playlist' });
    fireEvent.change(within(dialog).getByRole('combobox', { name: 'Artist name' }), { target: { value: 'Air' } });
    expect(await within(dialog).findByRole('list', { name: 'First tracks' }, { timeout: 3000 })).toBeVisible();
    expect(apiMock.playlists.smartPreview).toHaveBeenCalledTimes(2);
    expect(within(dialog).queryByText('Couldn’t update the preview.')).toBeNull();
  });

  it('renames on save and explains save failures', async () => {
    apiMock.playlists.smartDefinition.mockResolvedValue(stored);
    apiMock.playlists.updateSmartDefinition
      .mockRejectedValueOnce(new ApiError('boom', 500, null, null))
      .mockRejectedValueOnce(new Error('A playlist with this name already exists'))
      .mockResolvedValueOnce({ ...stored, editRevision: 4 })
      .mockRejectedValueOnce(new ApiError('newer', 409, 'unsupported', null));
    apiMock.playlists.update.mockResolvedValue({ ...smartPlaylist, name: 'Later night' });
    const onSaved = vi.fn();
    const onClose = vi.fn();
    render(<SmartPlaylistEditor playlist={smartPlaylist} existingNames={['Late night']} onClose={onClose} onSaved={onSaved} />);
    const dialog = screen.getByRole('dialog', { name: 'Edit rules' });
    await within(dialog).findByRole('combobox', { name: 'Genre' });

    fireEvent.change(within(dialog).getByLabelText('Playlist name'), { target: { value: '  ' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save rules' }));
    expect(within(dialog).getByLabelText('Playlist name')).toHaveFocus();

    fireEvent.change(within(dialog).getByLabelText('Playlist name'), { target: { value: 'Later night' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save rules' }));
    expect(await within(dialog).findByText('Could not save the rules.')).toBeVisible();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save rules' }));
    expect(await within(dialog).findByText('A playlist with this name already exists')).toBeVisible();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save rules' }));
    await waitFor(() => expect(onSaved).toHaveBeenCalledWith(expect.objectContaining({ name: 'Later night' })));
    expect(apiMock.playlists.update).toHaveBeenCalledWith('p1', 'Later night', '');

    fireEvent.click(within(dialog).getByRole('button', { name: 'Save rules' }));
    expect(await within(dialog).findByText("These rules were made by a newer version of BoogieBox and can’t be edited here.")).toBeVisible();

    fireEvent(dialog, new Event('cancel', { cancelable: true }));
    fireEvent.click(dialog);
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it('loads the newer rules after an edit conflict', async () => {
    apiMock.playlists.smartDefinition
      .mockResolvedValueOnce(stored)
      .mockResolvedValueOnce({
        ...stored, editRevision: 5,
        definition: { ...stored.definition, root: { mode: 'all', children: [{ field: 'genre', operator: 'is', value: 'Jazz' }] } },
      });
    apiMock.playlists.updateSmartDefinition.mockRejectedValueOnce(new ApiError('changed', 409, 'edit_conflict', null));
    render(<SmartPlaylistEditor playlist={smartPlaylist} existingNames={[]} onClose={vi.fn()} onSaved={vi.fn()} />);
    const dialog = screen.getByRole('dialog', { name: 'Edit rules' });
    await within(dialog).findByRole('combobox', { name: 'Genre' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save rules' }));
    fireEvent.click(await within(dialog).findByRole('button', { name: 'Load the newer rules' }));
    await waitFor(() => expect(within(dialog).getByRole('combobox', { name: 'Genre' })).toHaveValue('Jazz'));
    expect(within(dialog).queryByText('These rules were changed on another device while you were editing.')).toBeNull();
  });

  it('starts from a Home preset and can create and play', async () => {
    const onSaved = vi.fn();
    const onCreateAndPlay = vi.fn();
    apiMock.playlists.createSmart.mockResolvedValue({ ...smartPlaylist, id: 'p9', name: 'Rock' });
    render(<SmartPlaylistEditor existingNames={[]} onClose={vi.fn()} onSaved={onSaved} onCreateAndPlay={onCreateAndPlay}
      preset={genrePreset(['Rock'])} />);
    const dialog = screen.getByRole('dialog', { name: 'New smart playlist' });
    expect(within(dialog).getByLabelText('Playlist name')).toHaveValue('Rock');
    expect(within(dialog).getByRole('combobox', { name: 'Field' })).toHaveValue('genre');
    expect(within(dialog).getByRole('combobox', { name: 'Genre' })).toHaveValue('Rock');
    expect(within(dialog).getByLabelText('Order by')).toHaveValue('added_at');
    expect(within(dialog).getByRole('button', { name: /Create playlist/ })).toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole('button', { name: /Create and play/ }));
    await waitFor(() => expect(onCreateAndPlay).toHaveBeenCalledWith(expect.objectContaining({ id: 'p9' })));
    expect(onSaved).not.toHaveBeenCalled();
    expect(apiMock.playlists.createSmart).toHaveBeenCalledWith('Rock', expect.objectContaining({
      root: { mode: 'all', children: [{ field: 'genre', operator: 'is', value: 'Rock' }] },
      sort: [{ field: 'added_at', direction: 'desc' }],
    }));
  });

  it('opens every idea preset as valid rules', () => {
    for (const idea of SMART_IDEAS) {
      const draft = definitionToDraft(idea.preset.definition, idea.preset.name);
      expect(draft, idea.id).not.toBeNull();
      expect(checkDraft(draft!).valid, idea.id).toBe(true);
    }
    expect(definitionToDraft(genrePreset(['Rock', 'Jazz']).definition, 'x')!.root.mode).toBe('any');
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

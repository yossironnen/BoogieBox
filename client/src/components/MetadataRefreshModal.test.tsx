/**
 * Tests Metadata Refresh Modal.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import MetadataRefreshModal from './MetadataRefreshModal';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    integrations: {
      metadataSearch: vi.fn(),
    },
    updateAlbumMetadata: vi.fn(),
    updateArtistMetadata: vi.fn(),
    refreshAlbumCover: vi.fn(),
    refreshArtistPhoto: vi.fn(),
  },
}));

vi.mock('../api', () => ({
  api: apiMock,
}));

describe('MetadataRefreshModal', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMock.refreshAlbumCover.mockResolvedValue({ ok: true });
    apiMock.refreshArtistPhoto.mockResolvedValue({ ok: true });
  });

  it('coerces string provider years before applying album metadata', async () => {
    apiMock.integrations.metadataSearch.mockResolvedValue({
      results: [{
        provider: 'discogs',
        type: 'album',
        title: 'Actually/Further Listening',
        artist: 'Pet Shop Boys',
        year: '2001',
        genre: 'Electronic',
      }],
    });
    apiMock.updateAlbumMetadata.mockResolvedValue({ ok: true });

    render(
      <MetadataRefreshModal
        mode="album"
        entityId="album-1"
        initialArtist="Pet Shop Boys"
        initialAlbum="Actually/Further Listening"
        onClose={vi.fn()}
        onApplied={vi.fn()}
      />
    );

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Apply' }));

    await waitFor(() => expect(apiMock.updateAlbumMetadata).toHaveBeenCalledWith(
      'album-1',
      expect.objectContaining({ year: 2001 }),
      true,
    ));
  });
});

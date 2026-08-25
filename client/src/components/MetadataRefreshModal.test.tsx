/**
 * Tests Metadata Refresh Modal.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import MetadataRefreshModal, { normalizeMetadataYear } from './MetadataRefreshModal';

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

  it('normalizes numeric, prefixed-string, invalid, and missing years', () => {
    expect(normalizeMetadataYear(2001.9)).toBe(2001);
    expect(normalizeMetadataYear(' 1999 remaster')).toBe(1999);
    expect(normalizeMetadataYear('unknown')).toBeUndefined();
    expect(normalizeMetadataYear(Number.POSITIVE_INFINITY)).toBeUndefined();
    expect(normalizeMetadataYear(undefined)).toBeUndefined();
  });

  it('searches and applies artist results across known and fallback providers', async () => {
    const onClose = vi.fn();
    const onApplied = vi.fn();
    apiMock.integrations.metadataSearch.mockResolvedValue({
      results: [
        { provider: 'lastfm', type: 'artist', title: 'New Artist', artist: 'Credit', year: 2020, genre: 'Rock', image: '/artist.jpg', tags: ['rock', 'indie'] },
        { provider: 'custom', type: 'artist', title: 'Second Artist', tags: ['electronic'] },
      ],
    });
    apiMock.updateArtistMetadata.mockResolvedValue({ ok: true });
    render(
      <MetadataRefreshModal
        mode="artist"
        entityId="artist-1"
        initialArtist="Old Artist"
        onClose={onClose}
        onApplied={onApplied}
      />,
    );

    fireEvent.keyDown(screen.getByDisplayValue('Old Artist'), { key: 'Enter' });
    expect(await screen.findByText('Last.fm')).toBeInTheDocument();
    expect(screen.getByText('custom')).toBeInTheDocument();
    expect(screen.getAllByText(/result/).some((node) => node.textContent === '1 result')).toBe(true);
    fireEvent.click(screen.getAllByRole('button', { name: 'Apply' })[0]);
    await waitFor(() => expect(apiMock.updateArtistMetadata).toHaveBeenCalledWith('artist-1', { name: 'New Artist' }, true));
    expect(apiMock.refreshArtistPhoto).toHaveBeenCalledWith('artist-1');
    expect(onApplied).toHaveBeenCalledWith();

    fireEvent.click(screen.getByRole('button', { name: '✕' }));
    expect(onClose).toHaveBeenCalled();
  });

  it('shows empty search, search failure, and apply failure fallbacks', async () => {
    apiMock.integrations.metadataSearch
      .mockResolvedValueOnce({ results: [] })
      .mockRejectedValueOnce({})
      .mockResolvedValueOnce({ results: [{ provider: 'spotify', type: 'album', title: 'Result', releaseType: 'single', tags: ['Pop'] }] });
    apiMock.updateAlbumMetadata.mockRejectedValue({});
    render(
      <MetadataRefreshModal
        mode="album"
        entityId="album-1"
        initialArtist="Artist"
        initialAlbum=""
        onClose={vi.fn()}
        onApplied={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    expect(await screen.findByText('No results found.')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    expect(await screen.findByText('Search failed')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Apply' }));
    expect(await screen.findByText('Apply failed')).toBeInTheDocument();
    expect(apiMock.updateAlbumMetadata).toHaveBeenCalledWith('album-1', expect.objectContaining({
      genre: 'Pop',
      releaseType: 'single',
      discogsReleaseType: undefined,
      spotifyReleaseType: 'single',
      year: undefined,
    }), true);
  });

  it('shows a rate-limit message when a provider was rate limited', async () => {
    apiMock.integrations.metadataSearch.mockResolvedValue({
      results: [],
      provider_warnings: [{ provider: 'spotify', reason: 'rate_limited' }],
    });
    render(
      <MetadataRefreshModal
        mode="artist"
        entityId="artist-1"
        initialArtist="Madonna"
        onClose={vi.fn()}
        onApplied={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    expect(await screen.findByText(/Metadata provider rate limit reached/)).toBeInTheDocument();
    expect(screen.getByText('No results found.')).toBeInTheDocument();
  });

  it('does not show the rate-limit message for a plain empty result', async () => {
    apiMock.integrations.metadataSearch.mockResolvedValue({ results: [], provider_warnings: [] });
    render(
      <MetadataRefreshModal
        mode="artist"
        entityId="artist-1"
        initialArtist="Madonna"
        onClose={vi.fn()}
        onApplied={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    expect(await screen.findByText('No results found.')).toBeInTheDocument();
    expect(screen.queryByText(/Metadata provider rate limit reached/)).not.toBeInTheDocument();
  });
});

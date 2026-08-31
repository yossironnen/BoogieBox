/**
 * Tests Art Image.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { act, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import ArtImage from './ArtImage';

describe('ArtImage', () => {
  it('renders an image immediately when eager', async () => {
    render(
      <ArtImage
        src="/cover.jpg"
        alt=""
        eager={true}
        imgStyle={{ width: 40, height: 40 }}
        fallback={<span>fallback</span>}
      />,
    );

    const image = await screen.findByRole('presentation');
    expect(image).toHaveAttribute('src', expect.stringContaining('/cover.jpg'));
  });

  it('renders fallback when src is missing', () => {
    render(
      <ArtImage
        src={null}
        alt=""
        imgStyle={{ width: 40, height: 40 }}
        fallback={<span>fallback</span>}
      />,
    );

    expect(screen.getByText('fallback')).toBeInTheDocument();
  });

  it('reports load-state changes', async () => {
    const states: string[] = [];
    render(
      <ArtImage
        src="/cover.jpg"
        alt=""
        eager={true}
        imgStyle={{ width: 40, height: 40 }}
        onLoadStateChange={(state) => states.push(state)}
      />,
    );

    const image = await screen.findByRole('presentation');
    image.dispatchEvent(new Event('load'));
    await waitFor(() => expect(states).toContain('loaded'));
  });

  describe('bounded retry on error (Phase 3.2 background-fill contract)', () => {
    beforeEach(() => {
      vi.useFakeTimers();
    });
    afterEach(() => {
      vi.useRealTimers();
    });

    it('retries a failed image load with a cache-busting param after a delay', async () => {
      render(
        <ArtImage
          src="/api/albums/1/art?size=300"
          alt=""
          eager={true}
          imgStyle={{ width: 40, height: 40 }}
          fallback={<span>fallback</span>}
        />,
      );

      const image = screen.getByRole('presentation');
      expect(image.getAttribute('src')).toBe('/api/albums/1/art?size=300');

      act(() => {
        image.dispatchEvent(new Event('error'));
      });
      // Not yet retried — still the fallback-less "pending" state.
      expect(screen.queryByText('fallback')).not.toBeInTheDocument();

      act(() => {
        vi.advanceTimersByTime(400);
      });

      expect(screen.getByRole('presentation').getAttribute('src')).toBe(
        '/api/albums/1/art?size=300&_retry=1',
      );
    });

    it('stops after a small fixed number of attempts and shows the fallback', async () => {
      render(
        <ArtImage
          src="/api/albums/1/art?size=300"
          alt=""
          eager={true}
          imgStyle={{ width: 40, height: 40 }}
          fallback={<span>fallback</span>}
        />,
      );

      // Exhaust every retry (4 configured delays: 400/800/1600/3200ms).
      for (const delay of [400, 800, 1600, 3200]) {
        const image = screen.getByRole('presentation');
        act(() => {
          image.dispatchEvent(new Event('error'));
        });
        act(() => {
          vi.advanceTimersByTime(delay);
        });
      }

      // One final error past the last retry must give up and show the fallback.
      act(() => {
        screen.getByRole('presentation').dispatchEvent(new Event('error'));
      });
      expect(screen.getByText('fallback')).toBeInTheDocument();
    });

    it('cancels a pending retry timer when the source changes', async () => {
      const { rerender } = render(
        <ArtImage
          src="/api/albums/1/art?size=300"
          alt=""
          eager={true}
          imgStyle={{ width: 40, height: 40 }}
        />,
      );

      const image = screen.getByRole('presentation');
      act(() => {
        image.dispatchEvent(new Event('error'));
      });

      rerender(
        <ArtImage
          src="/api/albums/2/art?size=300"
          alt=""
          eager={true}
          imgStyle={{ width: 40, height: 40 }}
        />,
      );

      act(() => {
        vi.advanceTimersByTime(400);
      });

      // The stale timer from album 1 must not have fired and rewritten the
      // now-current album 2 source.
      expect(screen.getByRole('presentation').getAttribute('src')).toBe(
        '/api/albums/2/art?size=300',
      );
    });

    it('cancels a pending retry timer on unmount', async () => {
      const { unmount } = render(
        <ArtImage
          src="/api/albums/1/art?size=300"
          alt=""
          eager={true}
          imgStyle={{ width: 40, height: 40 }}
        />,
      );

      const image = screen.getByRole('presentation');
      act(() => {
        image.dispatchEvent(new Event('error'));
      });

      unmount();

      // Must not throw ("update on an unmounted component") when the timer
      // that was scheduled before unmount would otherwise have fired.
      expect(() => act(() => vi.advanceTimersByTime(5000))).not.toThrow();
    });
  });
});

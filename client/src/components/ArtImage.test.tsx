/**
 * Tests Art Image.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
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
});

import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import VinylTurntable from './VinylTurntable';

describe('VinylTurntable', () => {
  it('renders artwork, clamps progress, and scrubs through pointer events', () => {
    const onSeek = vi.fn();
    const onSeekStart = vi.fn();
    const onSeekEnd = vi.fn();
    const { container, rerender, unmount } = render(
      <VinylTurntable
        albumArtUrl="/cover.jpg"
        title="Track"
        isPlaying
        currentTime={200}
        duration={100}
        onSeek={onSeek}
        onSeekStart={onSeekStart}
        onSeekEnd={onSeekEnd}
      />,
    );
    expect(screen.getByRole('group', { name: 'Track vinyl turntable, playing' })).toBeInTheDocument();
    expect(screen.getByAltText('Track vinyl')).toBeInTheDocument();
    expect(screen.getByTitle('Vinyl platter')).toHaveStyle({ animationPlayState: 'running' });
    const wrap = container.firstElementChild as HTMLDivElement;
    vi.spyOn(wrap, 'getBoundingClientRect').mockReturnValue({
      x: 0, y: 0, left: 0, top: 0, right: 130, bottom: 142, width: 130, height: 142,
      toJSON: () => ({}),
    });
    const arm = screen.getByTitle('Drag needle to seek');

    fireEvent.pointerDown(arm, { clientX: 70, clientY: 4 });
    expect(onSeekStart).toHaveBeenCalled();
    expect(onSeek).toHaveBeenCalledWith(0, false);
    fireEvent.pointerMove(window, { clientX: 67, clientY: 22 });
    expect(onSeek).toHaveBeenLastCalledWith(expect.any(Number), false);
    fireEvent.pointerUp(window, { clientX: 64, clientY: 41 });
    expect(onSeekEnd).toHaveBeenCalledWith(100);
    fireEvent.pointerMove(window, { clientX: 70, clientY: 4 });
    fireEvent.pointerUp(window, { clientX: 70, clientY: 4 });

    rerender(
      <VinylTurntable
        albumArtUrl={null}
        title="Track"
        isPlaying={false}
        currentTime={-10}
        duration={100}
        onSeek={onSeek}
      />,
    );
    expect(screen.queryByAltText('Track vinyl')).not.toBeInTheDocument();
    expect(screen.getByTitle('Vinyl platter')).toHaveStyle({ animationPlayState: 'paused' });
    unmount();
  });

  it('disables seeking for invalid duration and Hardcore Vinyl mode', () => {
    const onSeek = vi.fn();
    const { rerender } = render(
      <VinylTurntable
        albumArtUrl={null}
        title="Track"
        isPlaying={false}
        currentTime={5}
        duration={Number.NaN}
        onSeek={onSeek}
      />,
    );
    fireEvent.pointerDown(screen.getByTitle('Drag needle to seek'), { clientX: 70, clientY: 4 });
    expect(onSeek).not.toHaveBeenCalled();

    rerender(
      <VinylTurntable
        albumArtUrl={null}
        title="Track"
        isPlaying={false}
        currentTime={5}
        duration={100}
        seekDisabled
        onSeek={onSeek}
      />,
    );
    fireEvent.pointerDown(screen.getByTitle('Seeking disabled in Hardcore Vinyl mode'), {
      clientX: 70,
      clientY: 4,
    });
    expect(onSeek).not.toHaveBeenCalled();
  });
});

import { fireEvent, render } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { DEFAULT_PARAMETRIC_BANDS } from '../audio/eq';
import EqCurveCanvas from './EqCurveCanvas';

function makeContext() {
  const gradient = { addColorStop: vi.fn() };
  return {
    clearRect: vi.fn(),
    save: vi.fn(),
    scale: vi.fn(),
    fillRect: vi.fn(),
    beginPath: vi.fn(),
    moveTo: vi.fn(),
    lineTo: vi.fn(),
    stroke: vi.fn(),
    fillText: vi.fn(),
    closePath: vi.fn(),
    createLinearGradient: vi.fn(() => gradient),
    fill: vi.fn(),
    arc: vi.fn(),
    restore: vi.fn(),
    globalAlpha: 1,
    fillStyle: '',
    strokeStyle: '',
    lineWidth: 1,
    font: '',
    textAlign: '',
  } as unknown as CanvasRenderingContext2D;
}

describe('EqCurveCanvas', () => {
  beforeEach(() => {
    Object.defineProperty(window, 'devicePixelRatio', { configurable: true, value: 2 });
    document.documentElement.style.setProperty('--surface', '#111111');
    document.documentElement.style.setProperty('--border', '#222222');
    document.documentElement.style.setProperty('--text', '#eeeeee');
    document.documentElement.style.setProperty('--text-muted', '#999999');
    document.documentElement.style.setProperty('--accent', '#abcdef');
  });

  it('draws the response and supports selecting and dragging gain and filter nodes', () => {
    const ctx = makeContext();
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(ctx);
    const setPointerCapture = vi.fn();
    const releasePointerCapture = vi.fn();
    Object.assign(HTMLCanvasElement.prototype, { setPointerCapture, releasePointerCapture });
    const onChange = vi.fn();
    const onSelectBand = vi.fn();
    const bands = [
      { ...DEFAULT_PARAMETRIC_BANDS[1], frequencyHz: 120, gainDb: 0 },
      { ...DEFAULT_PARAMETRIC_BANDS[2], type: 'highpass' as const, frequencyHz: 1000, enabled: false },
    ];
    const { container, rerender } = render(
      <EqCurveCanvas
        bands={bands}
        selectedBandIndex={0}
        accentColor="var(--accent)"
        onChange={onChange}
        onSelectBand={onSelectBand}
        width={536}
        height={160}
      />,
    );
    const canvas = container.querySelector('canvas')!;
    vi.spyOn(canvas, 'getBoundingClientRect').mockReturnValue({
      x: 0, y: 0, left: 0, top: 0, right: 536, bottom: 160, width: 536, height: 160,
      toJSON: () => ({}),
    });

    expect(canvas.width).toBe(1072);
    expect(canvas.height).toBe(320);
    expect(ctx.fillText).toHaveBeenCalled();
    expect(ctx.arc).toHaveBeenCalledTimes(4);

    fireEvent.pointerDown(canvas, { clientX: 158, clientY: 74, pointerId: 7 });
    expect(onSelectBand).toHaveBeenCalledWith(0);
    fireEvent.pointerMove(canvas, { clientX: 208, clientY: 54, pointerId: 7 });
    expect(onChange).toHaveBeenCalledWith(0, {
      frequencyHz: expect.any(Number),
      gainDb: expect.any(Number),
    });
    fireEvent.pointerUp(canvas, { pointerId: 7 });
    expect(releasePointerCapture).toHaveBeenCalled();

    fireEvent.pointerDown(canvas, { clientX: 311, clientY: 74, pointerId: 8 });
    fireEvent.pointerMove(canvas, { clientX: 340, clientY: 20, pointerId: 8 });
    expect(onChange).toHaveBeenLastCalledWith(1, {
      frequencyHz: expect.any(Number),
    });

    fireEvent.pointerUp(canvas, { pointerId: 8 });
    fireEvent.pointerMove(canvas, { clientX: 400, clientY: 100, pointerId: 8 });
    fireEvent.pointerDown(canvas, { clientX: 500, clientY: 140, pointerId: 9 });

    rerender(
      <EqCurveCanvas
        bands={bands}
        selectedBandIndex={1}
        accentColor="#ff00ff"
        onChange={onChange}
        onSelectBand={onSelectBand}
      />,
    );
    expect(ctx.createLinearGradient).toHaveBeenCalled();
    expect(setPointerCapture).toHaveBeenCalled();
  });

  it('does not throw when a 2D rendering context is unavailable', () => {
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
    const { container } = render(
      <EqCurveCanvas
        bands={[]}
        selectedBandIndex={-1}
        accentColor=""
        onChange={vi.fn()}
        onSelectBand={vi.fn()}
      />,
    );
    expect(container.querySelector('canvas')).toBeInTheDocument();
  });
});

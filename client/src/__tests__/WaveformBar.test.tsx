/**
 * Tests Waveform Bar.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import WaveformBar, {
  computeWaveformTimeFromClientX,
  downsampleWaveform,
  formatWaveformTime,
} from '../components/WaveformBar';

function mockRect(element: HTMLElement, left = 0, width = 200): void {
  vi.spyOn(element, 'getBoundingClientRect').mockReturnValue({
    x: left,
    y: 0,
    width,
    height: 32,
    top: 0,
    right: left + width,
    bottom: 32,
    left,
    toJSON: () => ({}),
  } as DOMRect);
}

describe('WaveformBar helpers', () => {
  it('formats waveform timestamps', () => {
    expect(formatWaveformTime(0)).toBe('0:00');
    expect(formatWaveformTime(65)).toBe('1:05');
    expect(formatWaveformTime(3671)).toBe('1:01:11');
  });

  it('maps cursor X to waveform time using bounds', () => {
    expect(computeWaveformTimeFromClientX(100, 0, 200, 120)).toBe(60);
    expect(computeWaveformTimeFromClientX(-50, 0, 200, 120)).toBe(0);
    expect(computeWaveformTimeFromClientX(400, 0, 200, 120)).toBe(120);
  });

  it('downsamples waveform peaks', () => {
    const bins = downsampleWaveform([0, 10, 40, 80, 120, 200, 255], 4);
    expect(bins).toHaveLength(4);
    expect(Math.max(...bins)).toBe(255);
  });
});

describe('WaveformBar component', () => {
  it('renders waveform bins and marks played/unplayed regions', () => {
    render(
      <WaveformBar
        points={[0, 32, 64, 96, 128, 196, 224, 255]}
        duration={100}
        currentTime={50}
        status="ready"
        onSeek={() => {}}
      />,
    );

    const bins = screen.getAllByTestId('waveform-bin');
    expect(bins.length).toBeGreaterThan(100);
    const activeCount = bins.filter((bin) => bin.getAttribute('data-active') === 'true').length;
    const inactiveCount = bins.length - activeCount;
    expect(activeCount).toBeGreaterThan(0);
    expect(inactiveCount).toBeGreaterThan(0);
  });

  it('seeks to clicked position and emits seek callbacks', () => {
    const onSeek = vi.fn();
    const onSeekStart = vi.fn();
    const onSeekEnd = vi.fn();

    render(
      <WaveformBar
        points={[0, 64, 128, 255]}
        duration={120}
        currentTime={0}
        status="ready"
        onSeek={onSeek}
        onSeekStart={onSeekStart}
        onSeekEnd={onSeekEnd}
      />,
    );

    const bar = screen.getByTestId('waveform-bar');
    mockRect(bar, 0, 200);
    fireEvent.pointerDown(bar, { clientX: 100, pointerId: 1 });
    fireEvent.pointerUp(bar, { clientX: 100, pointerId: 1 });

    expect(onSeekStart).toHaveBeenCalledTimes(1);
    expect(onSeek).toHaveBeenCalled();
    expect(onSeekEnd).toHaveBeenCalledTimes(1);
    expect(onSeekEnd.mock.calls[0][0]).toBeCloseTo(60, 0);
  });

  it('scrubs continuously while dragging', () => {
    const onSeek = vi.fn();
    const onSeekEnd = vi.fn();

    render(
      <WaveformBar
        points={[0, 64, 128, 255]}
        duration={120}
        currentTime={0}
        status="ready"
        onSeek={onSeek}
        onSeekEnd={onSeekEnd}
      />,
    );

    const bar = screen.getByTestId('waveform-bar');
    mockRect(bar, 0, 200);
    fireEvent.pointerDown(bar, { clientX: 20, pointerId: 1 });
    fireEvent.pointerMove(bar, { clientX: 150, pointerId: 1 });
    fireEvent.pointerUp(bar, { clientX: 180, pointerId: 1 });

    expect(onSeek.mock.calls.length).toBeGreaterThanOrEqual(3);
    const finalSeek = onSeekEnd.mock.calls[0][0];
    expect(finalSeek).toBeCloseTo(108, 0);
  });

  it('shows hover marker and tooltip with target timestamp', () => {
    render(
      <WaveformBar
        points={[0, 64, 128, 255]}
        duration={120}
        currentTime={0}
        status="ready"
        onSeek={() => {}}
      />,
    );

    const bar = screen.getByTestId('waveform-bar');
    mockRect(bar, 0, 200);
    fireEvent.pointerMove(bar, { clientX: 75, pointerId: 1 });

    expect(screen.getByTestId('waveform-hover-marker')).toBeInTheDocument();
    expect(screen.getByTestId('waveform-tooltip').textContent).toBe('0:45');
  });

  it('renders section-band strip when sections are provided', () => {
    render(
      <WaveformBar
        points={null}
        duration={200}
        currentTime={0}
        status="ready"
        onSeek={() => {}}
        sections={[
          { kind: 'intro', start: 0, end: 20, confidence: 0.9, vocalDensity: 0, drumDensity: 0, energy: 0.2 },
          { kind: 'verse', start: 20, end: 80, confidence: 0.9, vocalDensity: 0.8, drumDensity: 0.7, energy: 0.6 },
          { kind: 'chorus', start: 80, end: 130, confidence: 0.9, vocalDensity: 1, drumDensity: 0.9, energy: 0.9 },
        ]}
      />,
    );
    expect(screen.getByTestId('section-band')).toBeInTheDocument();
    expect(screen.getAllByTestId('section-segment')).toHaveLength(3);
  });

  it('does not render section-band when sections prop is absent', () => {
    render(
      <WaveformBar
        points={null}
        duration={200}
        currentTime={0}
        status="ready"
        onSeek={() => {}}
      />,
    );
    expect(screen.queryByTestId('section-band')).toBeNull();
  });

  it('renders transition window markers when transitionWindows are provided', () => {
    render(
      <WaveformBar
        points={null}
        duration={200}
        currentTime={0}
        status="ready"
        onSeek={() => {}}
        transitionWindows={[
          { role: 'intro', start: 0, end: 20, score: 0.8, vocalRisk: 0.1, drumContinuity: 0.9, bassRisk: 0.1, energy: 0.3, recommended: true },
          { role: 'outro', start: 170, end: 200, score: 0.7, vocalRisk: 0.1, drumContinuity: 0.5, bassRisk: 0.2, energy: 0.4, recommended: false },
        ]}
      />,
    );
    expect(screen.getAllByTestId('transition-window-marker')).toHaveLength(2);
  });
});

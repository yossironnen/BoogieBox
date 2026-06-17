/**
 * Tests SonicFingerprintPanel component rendering for BoogieBox regressions.
 */

import React from 'react';
import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import SonicFingerprintPanel from '../components/SonicFingerprintPanel';
import type { SonicFingerprint } from '../types';

const makeFingerprint = (overrides?: Partial<SonicFingerprint>): SonicFingerprint => ({
  trackId: 'track-1',
  bpmDetected: 128,
  energyScoreRefined: 0.75,
  confidence: 0.88,
  sourceDurationSec: 210,
  demucsModel: 'htdemucs',
  usedGpu: false,
  analysisSchemaVersion: 2,
  sectionJson: [
    { kind: 'intro', start: 0, end: 20, confidence: 0.9, vocalDensity: 0, drumDensity: 0.2, energy: 0.2 },
    { kind: 'verse', start: 20, end: 80, confidence: 0.85, vocalDensity: 0.8, drumDensity: 0.7, energy: 0.6 },
    { kind: 'chorus', start: 80, end: 130, confidence: 0.92, vocalDensity: 1, drumDensity: 0.9, energy: 0.9 },
    { kind: 'outro', start: 190, end: 210, confidence: 0.8, vocalDensity: 0.1, drumDensity: 0.3, energy: 0.2 },
  ],
  vocalWindowsJson: [{ start: 20, end: 130, strength: 0.85, average: 0.7 }],
  drumWindowsJson: [{ start: 0, end: 210, strength: 0.7, average: 0.6 }],
  bassWindowsJson: [{ start: 10, end: 180, strength: 0.6, average: 0.5 }],
  transitionWindowsJson: [
    { role: 'intro', start: 0, end: 20, score: 0.9, vocalRisk: 0.05, drumContinuity: 0.8, bassRisk: 0.1, energy: 0.2, recommended: true },
  ],
  introOutroRefinedJson: { introEnd: 20, outroStart: 190 },
  phraseBoundariesJson: [0, 16, 32, 48, 64, 80, 96, 112, 128],
  ...overrides,
});

describe('SonicFingerprintPanel', () => {
  it('renders badge row with BPM, energy, and confidence', () => {
    render(
      <SonicFingerprintPanel
        fingerprint={makeFingerprint()}
        waveformPoints={null}
        waveformStatus="missing"
        duration={210}
        currentTime={0}
        onSeek={() => {}}
        onClose={() => {}}
      />,
    );
    expect(screen.getByTestId('sfp-bpm-badge').textContent).toContain('128');
    expect(screen.getByTestId('sfp-energy-badge').textContent).toContain('75%');
    expect(screen.getByTestId('sfp-confidence-badge').textContent).toContain('88%');
  });

  it('renders three stem heatmap rows', () => {
    render(
      <SonicFingerprintPanel
        fingerprint={makeFingerprint()}
        waveformPoints={null}
        waveformStatus="missing"
        duration={210}
        currentTime={0}
        onSeek={() => {}}
        onClose={() => {}}
      />,
    );
    expect(screen.getByTestId('stem-row-vocals')).toBeInTheDocument();
    expect(screen.getByTestId('stem-row-drums')).toBeInTheDocument();
    expect(screen.getByTestId('stem-row-bass')).toBeInTheDocument();
  });

  it('renders stem rows without energy curve', () => {
    render(
      <SonicFingerprintPanel
        fingerprint={makeFingerprint()}
        waveformPoints={null}
        waveformStatus="missing"
        duration={210}
        currentTime={0}
        onSeek={() => {}}
        onClose={() => {}}
      />,
    );
    expect(screen.getByTestId('stem-row-vocals')).toBeInTheDocument();
    expect(screen.getByTestId('stem-row-bass')).toBeInTheDocument();
    expect(screen.queryByTestId('sfp-energy-curve')).not.toBeInTheDocument();
  });

  it('calls onClose when close button is clicked', () => {
    const onClose = vi.fn();
    render(
      <SonicFingerprintPanel
        fingerprint={makeFingerprint()}
        waveformPoints={null}
        waveformStatus="missing"
        duration={210}
        currentTime={0}
        onSeek={() => {}}
        onClose={onClose}
      />,
    );
    fireEvent.click(screen.getByTestId('sfp-close-button'));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('omits BPM badge when bpmDetected is null', () => {
    render(
      <SonicFingerprintPanel
        fingerprint={makeFingerprint({ bpmDetected: null })}
        waveformPoints={null}
        waveformStatus="missing"
        duration={210}
        currentTime={0}
        onSeek={() => {}}
        onClose={() => {}}
      />,
    );
    expect(screen.queryByTestId('sfp-bpm-badge')).toBeNull();
    expect(screen.getByTestId('sfp-energy-badge')).toBeInTheDocument();
  });

  it('renders the full panel container', () => {
    render(
      <SonicFingerprintPanel
        fingerprint={makeFingerprint()}
        waveformPoints={null}
        waveformStatus="missing"
        duration={210}
        currentTime={0}
        onSeek={() => {}}
        onClose={() => {}}
      />,
    );
    expect(screen.getByTestId('sonic-fingerprint-panel')).toBeInTheDocument();
  });
});

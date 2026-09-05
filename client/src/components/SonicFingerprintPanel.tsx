import React, { useMemo } from 'react';
import type { SonicFingerprint, StemWindow, TrackSection } from '../types';
import { hybridAudioPanelStyles, hybridControlStyles } from '../hybridPreview';
import WaveformBar, { type WaveformBarStatus } from './WaveformBar';

const DISPLAY_BINS = 180;

const SECTION_LABEL_COLORS: Record<string, string> = {
  intro: 'var(--text-faint)',
  verse: 'var(--accent)',
  chorus: 'var(--warning)',
  breakdown: 'var(--info)',
  build: 'color-mix(in srgb, var(--accent) 58%, var(--danger))',
  drop: 'var(--danger)',
  outro: 'var(--text-faint)',
};

const STEM_CONFIG = [
  { key: 'vocalWindowsJson' as const, label: 'VOCALS', color: 'var(--danger)' },
  { key: 'drumWindowsJson'  as const, label: 'DRUMS',  color: 'var(--warning)' },
  { key: 'bassWindowsJson'  as const, label: 'BASS',   color: 'var(--info)' },
];

function clamp(v: number, lo: number, hi: number) {
  return Math.max(lo, Math.min(hi, v));
}

function buildStemBins(windows: StemWindow[], duration: number, bins: number): number[] {
  if (!duration || !windows.length) return new Array<number>(bins).fill(0);
  const out = new Array<number>(bins).fill(0);
  for (let i = 0; i < bins; i++) {
    const binStart = (i / bins) * duration;
    const binEnd = ((i + 1) / bins) * duration;
    let maxStrength = 0;
    for (const w of windows) {
      if (w.end > binStart && w.start < binEnd) {
        maxStrength = Math.max(maxStrength, clamp(w.strength, 0, 1));
      }
    }
    out[i] = maxStrength;
  }
  return out;
}


export interface SonicFingerprintPanelProps {
  fingerprint: SonicFingerprint;
  waveformPoints: number[] | null;
  waveformStatus: WaveformBarStatus;
  duration: number;
  currentTime: number;
  onSeek: (time: number) => void;
  onSeekStart?: () => void;
  onSeekEnd?: (time: number) => void;
  onClose: () => void;
}

export default function SonicFingerprintPanel({
  fingerprint,
  waveformPoints,
  waveformStatus,
  duration,
  currentTime,
  onSeek,
  onSeekStart,
  onSeekEnd,
  onClose,
}: SonicFingerprintPanelProps) {
  const dur = duration || fingerprint.sourceDurationSec || 0;

  const stemBins = useMemo(() => ({
    vocal: buildStemBins(fingerprint.vocalWindowsJson, dur, DISPLAY_BINS),
    drums: buildStemBins(fingerprint.drumWindowsJson, dur, DISPLAY_BINS),
    bass:  buildStemBins(fingerprint.bassWindowsJson, dur, DISPLAY_BINS),
  }), [fingerprint, dur]);

  const playedRatio = dur > 0 ? clamp(currentTime / dur, 0, 1) : 0;

  const bpmLabel = fingerprint.bpmDetected != null
    ? `♩ ${Math.round(fingerprint.bpmDetected)} BPM`
    : null;
  const energyLabel = `⚡ Energy ${Math.round(fingerprint.energyScoreRefined * 100)}%`;
  const confLabel = `◎ Confidence ${Math.round(fingerprint.confidence * 100)}%`;

  return (
    <div
      data-testid="sonic-fingerprint-panel"
      style={{
        ...hybridAudioPanelStyles.fingerprint,
        display: 'flex',
        flexDirection: 'column',
        gap: 10,
        boxSizing: 'border-box',
      }}
    >
      {/* Header row */}
      <div style={hybridAudioPanelStyles.fingerprintHeader}>
        <span style={{
          fontSize: 12,
          fontWeight: 750,
          letterSpacing: '0.09em',
          color: 'var(--accent)',
          textTransform: 'uppercase' as const,
        }}>
          Sonic Fingerprint ✦
        </span>
        <div style={{ flex: 1 }} />
        {/* Badges */}
        {bpmLabel && <Badge label={bpmLabel} testId="sfp-bpm-badge" title="Beats per minute — detected by AI stem analysis" />}
        <Badge label={energyLabel} testId="sfp-energy-badge" title="Overall energy score derived from vocal, drum, and bass stem activity (0–100%)" />
        <Badge label={confLabel} testId="sfp-confidence-badge" title="How reliable the AI stem analysis is. Values below 30% indicate synthetic fallback data." />
        {fingerprint.demucsModel && (
          <Badge label={fingerprint.demucsModel} testId="sfp-model-badge" title="AI model used for stem separation" />
        )}
        <button
          data-testid="sfp-close-button"
          type="button"
          onClick={onClose}
          style={{ ...hybridControlStyles.iconButton, width: 30, minWidth: 30, height: 30, marginLeft: 2 }}
          aria-label="Close Sonic Fingerprint"
        >
          ×
        </button>
      </div>

      {/* Waveform + section band — left-offset matches stem label width so bars align */}
      <div style={{ display: 'flex', alignItems: 'center', paddingLeft: 50 }}>
        <WaveformBar
          points={waveformPoints}
          duration={dur}
          currentTime={currentTime}
          status={waveformStatus}
          onSeek={onSeek}
          onSeekStart={onSeekStart}
          onSeekEnd={onSeekEnd}
          sections={fingerprint.sectionJson}
          transitionWindows={fingerprint.transitionWindowsJson}
        />
      </div>

      {/* Section legend — same left offset as waveform/stem bars */}
      {fingerprint.sectionJson.length > 0 && (
        <div style={{ paddingLeft: 50 }}>
          <SectionLegend sections={fingerprint.sectionJson} />
        </div>
      )}

      {/* Stem heatmap rows */}
      <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
        {STEM_CONFIG.map(({ key, label, color }) => {
          const bins = key === 'vocalWindowsJson' ? stemBins.vocal
                     : key === 'drumWindowsJson'  ? stemBins.drums
                     : stemBins.bass;
          return (
            <StemRow
              key={key}
              label={label}
              color={color}
              bins={bins}
              playedRatio={playedRatio}
            />
          );
        })}
      </div>

    </div>
  );
}

function Badge({ label, testId, title }: { label: string; testId?: string; title?: string }) {
  return (
    <span
      data-testid={testId}
      title={title}
      style={{
        ...hybridAudioPanelStyles.badge,
        whiteSpace: 'nowrap' as const,
        fontVariantNumeric: 'tabular-nums',
      }}
    >
      {label}
    </span>
  );
}

function StemRow({
  label,
  color,
  bins,
  playedRatio,
}: {
  label: string;
  color: string;
  bins: number[];
  playedRatio: number;
}) {
  const playedBins = Math.round(playedRatio * bins.length);
  return (
    <div
      data-testid={`stem-row-${label.toLowerCase()}`}
      style={{ display: 'flex', alignItems: 'center', gap: 6 }}
    >
      <span style={{
        fontSize: 12,
        fontWeight: 700,
        letterSpacing: '0.08em',
        color,
        width: 44,
        flexShrink: 0,
        textAlign: 'right' as const,
        opacity: 0.85,
      }}>
        {label}
      </span>
      <div style={{ flex: 1, height: 14, display: 'flex', alignItems: 'center', gap: 0.5 }}>
        {bins.map((strength, i) => (
          <div
            key={i}
            style={{
              flex: 1,
              minWidth: 1,
              height: `${Math.max(2, strength * 14)}px`,
              borderRadius: 1,
              backgroundColor: color,
              opacity: i < playedBins
                ? clamp(0.4 + strength * 0.6, 0.35, 1)
                : clamp(0.18 + strength * 0.55, 0.12, 0.7),
            }}
          />
        ))}
      </div>
    </div>
  );
}

function SectionLegend({ sections }: { sections: TrackSection[] }) {
  const seen = new Set<string>();
  const unique = sections.filter(s => {
    const k = s.kind.toLowerCase();
    if (seen.has(k)) return false;
    seen.add(k);
    return true;
  });
  if (unique.length === 0) return null;
  return (
    <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap' as const }}>
      {unique.map(s => {
        const color = SECTION_LABEL_COLORS[s.kind.toLowerCase()] ?? 'var(--text-muted)';
        return (
          <span key={s.kind} style={{ fontSize: 12, color, fontWeight: 600, display: 'flex', alignItems: 'center', gap: 4 }}>
            <span style={{ width: 7, height: 7, borderRadius: 1.5, backgroundColor: color, display: 'inline-block' }} />
            {s.kind}
          </span>
        );
      })}
    </div>
  );
}

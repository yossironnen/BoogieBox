import React, { useMemo } from 'react';
import type { SonicFingerprint, StemWindow, TrackSection } from '../types';
import WaveformBar, { type WaveformBarStatus } from './WaveformBar';

const DISPLAY_BINS = 180;

const SECTION_LABEL_COLORS: Record<string, string> = {
  intro: '#9e9e9e',
  verse: 'var(--accent)',
  chorus: '#f5a623',
  breakdown: '#7b61ff',
  build: '#e91e63',
  drop: '#ff5722',
  outro: '#9e9e9e',
};

const STEM_CONFIG = [
  { key: 'vocalWindowsJson' as const, label: 'VOCALS', color: '#e91e63' },
  { key: 'drumWindowsJson'  as const, label: 'DRUMS',  color: '#ff9800' },
  { key: 'bassWindowsJson'  as const, label: 'BASS',   color: '#2196f3' },
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
        width: '100%',
        background: 'var(--surface)',
        borderTop: '2px solid var(--accent)',
        boxShadow: '0 -4px 24px color-mix(in srgb, var(--accent) 18%, transparent), inset 0 1px 0 color-mix(in srgb, var(--accent) 20%, transparent)',
        padding: '10px 14px 14px',
        display: 'flex',
        flexDirection: 'column',
        gap: 9,
        boxSizing: 'border-box',
      }}
    >
      {/* Header row */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <span style={{
          fontSize: 10,
          fontWeight: 700,
          letterSpacing: '0.12em',
          color: 'var(--accent)',
          textTransform: 'uppercase' as const,
          opacity: 0.9,
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
          onClick={onClose}
          style={{
            background: 'none',
            border: 'none',
            cursor: 'pointer',
            color: 'var(--text-muted)',
            fontSize: 16,
            padding: '0 4px',
            lineHeight: 1,
            marginLeft: 2,
          }}
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
        fontSize: 10,
        lineHeight: '16px',
        padding: '2px 8px',
        borderRadius: 4,
        border: '1px solid color-mix(in srgb, var(--accent) 35%, var(--border))',
        backgroundColor: 'color-mix(in srgb, var(--accent) 10%, var(--bg))',
        color: 'var(--text)',
        whiteSpace: 'nowrap' as const,
        fontVariantNumeric: 'tabular-nums',
        fontWeight: 600,
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
        fontSize: 10,
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
          <span key={s.kind} style={{ fontSize: 10, color, fontWeight: 600, display: 'flex', alignItems: 'center', gap: 4 }}>
            <span style={{ width: 7, height: 7, borderRadius: 1.5, backgroundColor: color, display: 'inline-block' }} />
            {s.kind}
          </span>
        );
      })}
    </div>
  );
}

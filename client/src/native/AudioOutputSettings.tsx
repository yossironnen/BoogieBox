/**
 * Settings › User Settings › Audio output (desktop app only). Saved by the desktop app on this PC, not per user.
 * Copy: `wip/native-hires-approved-copy.md`.
 */

import React from 'react';
import { hybridSettingsStyles } from '../hybridPreview';
import { ToggleRow } from '../components/settingsControls';
import { deviceRateLong } from './nativeFormat';
import { LockIcon, SharedIcon, SineIcon, SpeakerIcon, TimerIcon, WaveIcon } from './NativeIcons';
import { activeDevice, useNativeAudio } from './useNativeAudio';
import type { NativeAudioSettings, NativeOutputMode } from './nativeTypes';

const BUFFERS: Array<{ ms: number; label: string }> = [
  { ms: 20, label: 'Low · 20 ms' },
  { ms: 50, label: 'Normal · 50 ms' },
  { ms: 150, label: 'Safe · 150 ms' },
];

const MODES: Array<{ mode: NativeOutputMode; title: string; text: string; Icon: typeof LockIcon }> = [
  {
    mode: 'exclusive',
    title: 'Exclusive',
    text: "Bit-perfect, at each file's own rate. Other apps are silent on this device while BoogieBox plays.",
    Icon: LockIcon,
  },
  {
    mode: 'shared',
    title: 'Shared',
    text: 'Plays alongside other apps. Windows mixes the sound and may resample it.',
    Icon: SharedIcon,
  },
];

const heading: React.CSSProperties = { display: 'flex', alignItems: 'center', gap: 8, fontSize: 15, fontWeight: 650, color: 'var(--text)', marginBottom: 8 };

function cardStyle(active: boolean): React.CSSProperties {
  return {
    display: 'flex', gap: 10, textAlign: 'left', padding: '12px 14px', borderRadius: 12, font: 'inherit', cursor: 'pointer',
    color: 'var(--text)',
    border: `1px solid ${active ? 'var(--accent)' : 'var(--border)'}`,
    background: active ? 'var(--accent-soft, color-mix(in srgb, var(--accent) 16%, transparent))' : 'var(--surface-subtle, var(--bg))',
  };
}

export default function AudioOutputSettings() {
  const native = useNativeAudio();
  const settings = native.settings;
  if (!native.available || !settings) return null;
  const update = (change: Partial<NativeAudioSettings>) => { void native.save({ ...settings, ...change }); };
  const defaultDevice = activeDevice(native.devices, { ...settings, deviceId: null });

  return (
    <section id="user-audio-output" aria-label="Audio output" style={hybridSettingsStyles.panel}>
      <div style={hybridSettingsStyles.panelHeader}>
        <div>
          <div style={hybridSettingsStyles.sectionTitle}>Audio output</div>
          <div style={hybridSettingsStyles.panelDescription}>How this PC plays when "This PC · Hi-Res" is the output.</div>
        </div>
      </div>

      <div role="radiogroup" aria-label="Device" style={{ marginBottom: 18 }}>
        <div style={heading}><SpeakerIcon />Device</div>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))', gap: 8 }}>
          <button type="button" role="radio" aria-checked={settings.deviceId == null} onClick={() => update({ deviceId: null })} style={{ ...cardStyle(settings.deviceId == null), flexDirection: 'column', gap: 3 }}>
            <span style={{ fontSize: 14, fontWeight: 700 }}>System default</span>
            <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>
              {defaultDevice ? `Follows Windows (now ${defaultDevice.name})` : 'Follows Windows'}
            </span>
          </button>
          {native.devices.map((device) => {
            const chosen = settings.deviceId === device.id;
            const detail = deviceRateLong(device);
            return (
              <button key={device.id} type="button" role="radio" aria-checked={chosen} onClick={() => update({ deviceId: device.id })} style={{ ...cardStyle(chosen), flexDirection: 'column', gap: 3 }}>
                <span style={{ fontSize: 14, fontWeight: 700 }}>{device.name}</span>
                {detail ? <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>{detail}</span> : null}
              </button>
            );
          })}
        </div>
      </div>

      <div role="radiogroup" aria-label="Mode" style={{ marginBottom: 18 }}>
        <div style={heading}><WaveIcon size={16} />Mode</div>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(240px, 1fr))', gap: 8 }}>
          {MODES.map(({ mode, title, text, Icon }) => {
            const chosen = settings.mode === mode;
            return (
              <button key={mode} type="button" role="radio" aria-checked={chosen} onClick={() => update({ mode })} style={cardStyle(chosen)}>
                <Icon style={{ flexShrink: 0, marginTop: 1, color: chosen ? 'var(--accent)' : 'var(--text-muted)' }} />
                <span style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
                  <span style={{ fontSize: 14, fontWeight: 700 }}>{title}</span>
                  <span style={{ fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.45 }}>{text}</span>
                </span>
              </button>
            );
          })}
        </div>
      </div>

      <div role="radiogroup" aria-label="Buffer" style={{ marginBottom: 18 }}>
        <div style={heading}><TimerIcon />Buffer</div>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
          {BUFFERS.map(({ ms, label }) => {
            const chosen = settings.bufferMs === ms;
            return (
              <button
                key={ms}
                type="button"
                role="radio"
                aria-checked={chosen}
                onClick={() => update({ bufferMs: ms })}
                style={{
                  padding: '8px 14px', borderRadius: 10, font: 'inherit', fontSize: 13, cursor: 'pointer',
                  fontWeight: chosen ? 700 : 600,
                  border: `1px solid ${chosen ? 'var(--accent)' : 'var(--border)'}`,
                  background: chosen ? 'var(--accent)' : 'transparent',
                  color: chosen ? 'var(--accent-contrast, #fff)' : 'var(--text)',
                }}
              >
                {label}
              </button>
            );
          })}
        </div>
        <div style={{ fontSize: 12, color: 'var(--text-muted)', marginTop: 6 }}>Choose Safe if you hear dropouts.</div>
      </div>

      <div style={{ display: 'flex', gap: 8 }}>
        <SineIcon size={16} style={{ flexShrink: 0, marginTop: 3, color: 'var(--text)' }} />
        <div style={{ flex: 1 }}>
          <ToggleRow
            title="Use the nearest rate the DAC can play"
            description="When the DAC can't play a file's sample rate, play it at the closest rate it supports. Off: such files play in Shared mode."
            checked={settings.nearestRate}
            onChange={() => update({ nearestRate: !settings.nearestRate })}
          />
        </div>
      </div>
      <div style={{ fontSize: 12, color: 'var(--text-muted)' }}>Saved on this PC, not per user.</div>
    </section>
  );
}

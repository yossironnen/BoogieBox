/**
 * Settings → Advanced → Play On (admin): switches, the speakers on the network with their display name, format
 * policy and visibility, the network interface override and discovery diagnostics. The section title lives in
 * SettingsPage. See wip/play-on-renderers-plan.md §9.
 */

import React, { useCallback, useEffect, useState } from 'react';
import { api, RemoteApiError } from '../api';
import { hybridAudioPanelStyles, hybridControlStyles } from '../hybridPreview';
import { RefreshIcon, RendererIcon } from '../remote/OutputIcon';
import type { RemoteDiagnostics, RemoteFormatPolicy, RendererInfo } from '../remote/remoteTypes';
import { parseServerDate } from '../utils';
import { InlineStatus, ToggleRow, ToggleSwitch } from './settingsControls';

const POLL_MS = 15000;
const SCAN_WAIT_MS = 2500;

export const PLAY_ON_FORMAT_OPTIONS: ReadonlyArray<{ value: RemoteFormatPolicy; label: string }> = [
  { value: 'auto', label: 'Auto' },
  { value: 'direct', label: 'Always original file' },
  { value: 'lossless', label: 'Lossless (WAV)' },
  { value: 'mp3', label: 'MP3 320' },
];

const ACTIVE_STATUSES = new Set(['playing', 'loading', 'paused', 'lostControl']);

function formatTime(ms: number | null): string | null {
  return ms ? new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : null;
}

/** "Linkplay · WiiM Ultra Receiver · gapless, seek" or "… · last seen …" for offline speakers. */
export function rendererDetail(renderer: RendererInfo): string {
  const parts = [renderer.manufacturer, renderer.model].filter(Boolean) as string[];
  if (renderer.online) {
    const caps = [
      renderer.capabilities.setNext ? 'gapless' : null,
      renderer.capabilities.seek ? 'seek' : null,
      renderer.capabilities.volume ? null : 'no volume control',
    ].filter(Boolean);
    if (caps.length) parts.push(caps.join(', '));
  } else {
    const seen = parseServerDate(renderer.lastSeenAt);
    if (seen && !Number.isNaN(seen.getTime())) parts.push(`last seen ${seen.toLocaleString()}`);
  }
  return parts.join(' · ');
}

function errorText(error: unknown): string {
  return error instanceof Error && error.message ? error.message : 'Could not save.';
}

function SpeakerRow({ renderer, onPatched }: { renderer: RendererInfo; onPatched: (message: string) => void }) {
  const [alias, setAlias] = useState(renderer.alias ?? '');
  useEffect(() => setAlias(renderer.alias ?? ''), [renderer.alias]);

  const patch = async (changes: Parameters<typeof api.remote.updateRenderer>[1]) => {
    try {
      await api.remote.updateRenderer(renderer.udn, changes);
      onPatched('Saved');
    } catch (error) {
      onPatched(`Error: ${errorText(error)}`);
    }
  };

  const commitAlias = () => {
    const next = alias.trim();
    if (next === (renderer.alias ?? '')) return;
    void patch({ alias: next ? next : null });
  };

  return (
    <div role="listitem" style={S.row}>
      <RendererIcon style={{ color: 'var(--text-muted)' }} />
      <div style={{ minWidth: 0, display: 'flex', flexDirection: 'column', gap: 3 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0 }}>
          <span style={S.rowName}>{renderer.name}</span>
          <span style={{ ...hybridAudioPanelStyles.badge, ...(renderer.online ? hybridAudioPanelStyles.badgeAccent : {}) }}>
            {renderer.online ? 'Online' : 'Offline'}
          </span>
        </div>
        <div style={S.rowDetail}>{rendererDetail(renderer)}</div>
      </div>
      <label style={S.fieldLabel}>
        Display name
        <input
          type="text"
          value={alias}
          placeholder={renderer.friendlyName}
          maxLength={80}
          aria-label={`Display name for ${renderer.friendlyName}`}
          onChange={(event) => setAlias(event.target.value)}
          onBlur={commitAlias}
          onKeyDown={(event) => { if (event.key === 'Enter') event.currentTarget.blur(); }}
          style={{ ...hybridControlStyles.field, minHeight: 38, padding: '7px 10px', fontSize: 14 }}
        />
      </label>
      <label style={S.fieldLabel}>
        Format
        <select
          value={renderer.formatPolicy}
          aria-label={`Format for ${renderer.name}`}
          onChange={(event) => void patch({ formatPolicy: event.target.value as RemoteFormatPolicy })}
          style={{ ...hybridControlStyles.select, background: 'var(--surface)' }}
        >
          {PLAY_ON_FORMAT_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
        </select>
      </label>
      <div style={{ ...S.fieldLabel, alignItems: 'flex-start', gap: 6 }}>
        Hidden
        <ToggleSwitch checked={renderer.hidden} ariaLabel={`Hide ${renderer.name}`} onChange={() => void patch({ hidden: !renderer.hidden })} />
      </div>
    </div>
  );
}

/** Card content for the Play On settings. */
export default function PlayOnSettings() {
  const [enabled, setEnabled] = useState(true);
  const [releaseStops, setReleaseStops] = useState(true);
  const [interfaceIp, setInterfaceIp] = useState('');
  const [renderers, setRenderers] = useState<RendererInfo[]>([]);
  const [diagnostics, setDiagnostics] = useState<RemoteDiagnostics | null>(null);
  const [result, setResult] = useState<string | null>(null);
  const [scanning, setScanning] = useState(false);

  const refresh = useCallback(() => {
    api.remote.renderers()
      .then((response) => setRenderers(response.renderers))
      .catch((error: unknown) => { if (error instanceof RemoteApiError && error.status === 404) setRenderers([]); });
    api.remote.diagnostics().then(setDiagnostics).catch(() => {});
  }, []);

  useEffect(() => {
    api.settings.get().then((s) => {
      setEnabled((s.remotePlaybackEnabled ?? 'true') !== 'false');
      setReleaseStops((s.remoteReleaseStopsRenderer ?? 'true') !== 'false');
      setInterfaceIp(s.remoteInterfaceIp ?? '');
    }).catch(() => {});
    refresh();
    const timer = window.setInterval(refresh, POLL_MS);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const save = async (updates: Record<string, string>) => {
    try {
      await api.settings.update(updates);
      setResult('Saved');
      // Discovery restarts on a settings change; give it a moment before re-reading.
      window.setTimeout(refresh, 1200);
    } catch (error) {
      setResult(`Error: ${errorText(error)}`);
    }
  };

  const scan = async () => {
    setScanning(true);
    setResult(null);
    try {
      await api.remote.refresh();
      await new Promise((resolve) => window.setTimeout(resolve, SCAN_WAIT_MS));
      const response = await api.remote.renderers();
      setRenderers(response.renderers);
      setResult(`Found ${response.renderers.length} ${response.renderers.length === 1 ? 'speaker' : 'speakers'}`);
    } catch (error) {
      setResult(`Error: ${errorText(error)}`);
    } finally {
      setScanning(false);
    }
  };

  const onPatched = (message: string) => {
    setResult(message);
    refresh();
  };

  const detected = diagnostics?.detectedInterfaces ?? [];
  const interfaceChoices = interfaceIp && !detected.includes(interfaceIp) ? [...detected, interfaceIp] : detected;
  const activeSessions = renderers.filter((r) => r.session && ACTIVE_STATUSES.has(r.session.status));
  const lastSearch = formatTime(diagnostics?.lastSearchMs ?? null);

  return (
    <div style={S.card}>
      <ToggleRow
        title="Enable Play On"
        checked={enabled}
        onChange={() => {
          const next = !enabled;
          setEnabled(next);
          void save({ remotePlaybackEnabled: next ? 'true' : 'false' });
        }}
        description="Find speakers and streamers on your network (UPnP/DLNA) and play to them."
      />
      <ToggleRow
        title="Stop the speaker when switching back to a browser"
        checked={releaseStops}
        onChange={() => {
          const next = !releaseStops;
          setReleaseStops(next);
          void save({ remoteReleaseStopsRenderer: next ? 'true' : 'false' });
        }}
        description="When playback moves back to a browser, the speaker stops instead of continuing on its own."
      />

      <div style={S.listHeader}>
        <div style={S.listTitle}>Speakers on your network</div>
        <button
          type="button"
          disabled={!enabled || scanning}
          onClick={() => void scan()}
          style={{ ...hybridControlStyles.secondaryButton, display: 'inline-flex', alignItems: 'center', gap: 7, ...(!enabled || scanning ? hybridControlStyles.disabled : {}) }}
        >
          <RefreshIcon style={scanning ? { animation: 'sidebarScanSpin 1.15s linear infinite' } : undefined} />
          Scan now
        </button>
      </div>
      {renderers.length ? (
        <div role="list" aria-label="Speakers" style={S.list}>
          {renderers.map((renderer) => <SpeakerRow key={renderer.udn} renderer={renderer} onPatched={onPatched} />)}
        </div>
      ) : (
        <div style={S.muted}>{enabled ? 'No speakers found yet.' : 'Play On is off.'}</div>
      )}
      <InlineStatus busy={scanning} busyText="Searching for speakers…" result={result} />

      <div style={S.interfaceRow}>
        <label style={{ display: 'flex', flexDirection: 'column', gap: 4, fontSize: 14, color: 'var(--text-muted)' }}>
          Network interface
          <select
            value={interfaceIp}
            onChange={(event) => {
              setInterfaceIp(event.target.value);
              void save({ remoteInterfaceIp: event.target.value });
            }}
            style={{ ...hybridControlStyles.select, minWidth: 320 }}
          >
            <option value="">{detected.length ? `Automatic (${detected.join(', ')})` : 'Automatic'}</option>
            {interfaceChoices.map((ip) => <option key={ip} value={ip}>{ip}</option>)}
          </select>
        </label>
        <div style={{ ...S.muted, paddingBottom: 8 }}>Pick one only if a speaker is on a different network adapter.</div>
      </div>

      {diagnostics ? (
        <div style={S.diagnostics} aria-label="Play On diagnostics">
          <div>
            {diagnostics.running
              ? `Searching on ${diagnostics.searchInterfaces.join(' and ') || 'no interface'}${lastSearch ? ` · last search ${lastSearch}` : ''}`
              : 'Discovery is not running'}
          </div>
          <div>
            {`Active sessions: ${activeSessions.length}`}
            {activeSessions.length
              ? ` (${activeSessions.map((r) => [r.name, r.session?.ownerName].filter(Boolean).join(', ')).join('; ')})`
              : ''}
          </div>
          <div style={diagnostics.lastError ? { color: 'var(--danger)' } : undefined}>{diagnostics.lastError ?? 'No errors'}</div>
        </div>
      ) : null}
    </div>
  );
}

const S: Record<string, React.CSSProperties> = {
  card: {
    padding: '16px 20px',
    borderRadius: 8,
    marginBottom: 12,
    backgroundColor: 'var(--surface)',
    border: '1px solid var(--border)',
  },
  listHeader: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
    margin: '4px 0 10px',
  },
  listTitle: { fontSize: 15, fontWeight: 600, color: 'var(--text)' },
  list: { display: 'flex', flexDirection: 'column', gap: 8, marginBottom: 8 },
  row: {
    display: 'grid',
    gridTemplateColumns: '28px minmax(0, 1fr) minmax(160px, 220px) minmax(150px, 190px) 70px',
    alignItems: 'center',
    gap: 14,
    padding: '12px 14px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 10,
    background: 'var(--surface-subtle)',
  },
  rowName: { fontSize: 15, fontWeight: 650, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  rowDetail: { fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.45, overflowWrap: 'anywhere' },
  fieldLabel: { display: 'flex', flexDirection: 'column', gap: 4, fontSize: 12, color: 'var(--text-muted)' },
  muted: { fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.6 },
  interfaceRow: { display: 'flex', alignItems: 'flex-end', flexWrap: 'wrap', gap: 16, margin: '18px 0' },
  diagnostics: {
    paddingTop: 14,
    borderTop: '1px solid var(--divider-subtle)',
    display: 'flex',
    flexDirection: 'column',
    gap: 4,
    fontSize: 13,
    color: 'var(--text-muted)',
  },
};

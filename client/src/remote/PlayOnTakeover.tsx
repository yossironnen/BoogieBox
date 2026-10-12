/**
 * Takeover confirmation copy and the desktop prompt (`ConfirmModal`), shown before replacing another user's
 * active renderer session (plan 5.5 / 8.6).
 */

import ConfirmModal from '../components/ConfirmModal';
import { UserIcon } from './OutputIcon';
import type { RemoteSessionStatus } from './remoteTypes';
import type { PlayOnTakeoverPrompt } from './usePlayOn';

const STATUS_LABEL: Partial<Record<RemoteSessionStatus, string>> = {
  playing: 'playing',
  loading: 'starting',
  paused: 'paused',
  lostControl: 'playing another source',
};

/** "{Owner} is listening on {Device}. …" — `peer` names where their playback goes back to. */
export function takeoverDescription(prompt: PlayOnTakeoverPrompt, peer: 'browser' | 'device'): string {
  const { ownerName, deviceName } = prompt.info;
  const owner = ownerName ?? 'Someone else';
  return prompt.replacesQueue
    ? `${owner} is listening on ${deviceName}. Your music replaces theirs, and their playback moves back to their ${peer}, paused.`
    : `${owner} is listening on ${deviceName}. Taking control moves their playback back to their ${peer}, paused.`;
}

export function takeoverTitle(prompt: PlayOnTakeoverPrompt): string {
  return `Take over ${prompt.info.deviceName}?`;
}

export function PlayOnTakeoverModal({ prompt }: { prompt: PlayOnTakeoverPrompt }) {
  const { nowPlaying, ownerName, status } = prompt.info;
  return (
    <ConfirmModal
      title={takeoverTitle(prompt)}
      message={(
        <span style={{ display: 'flex', flexDirection: 'column', gap: 14, whiteSpace: 'normal' }}>
          <span>{takeoverDescription(prompt, 'browser')}</span>
          {nowPlaying?.title ? (
            <span style={{ display: 'flex', alignItems: 'center', gap: 12, padding: '10px 12px', border: '1px solid var(--divider-subtle)', borderRadius: 10, background: 'var(--surface-subtle)' }}>
              <span style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column' }}>
                <span style={{ fontSize: 14, fontWeight: 650, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{nowPlaying.title}</span>
                {STATUS_LABEL[status] ? <span style={{ fontSize: 12 }}>{STATUS_LABEL[status]}</span> : null}
              </span>
              {ownerName ? (
                <span style={{ display: 'inline-flex', alignItems: 'center', gap: 5, fontSize: 12 }}>
                  <UserIcon />
                  {ownerName}
                </span>
              ) : null}
            </span>
          ) : null}
        </span>
      )}
      confirmLabel="Take over"
      cancelLabel="Cancel"
      busy={prompt.busy}
      onConfirm={() => { void prompt.confirm(); }}
      onCancel={prompt.cancel}
    />
  );
}

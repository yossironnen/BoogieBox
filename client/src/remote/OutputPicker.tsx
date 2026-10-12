/**
 * Play On picker rows ("This browser" first, then renderers), shared by the desktop dock popup and the mobile
 * Now Playing sheet. Built from the queue panel's row styles (plan 8.6).
 */

import React, { useEffect, useRef } from 'react';
import { hybridAudioPanelStyles, hybridControlStyles } from '../hybridPreview';
import { BrowserIcon, CheckIcon, PhoneIcon, RendererIcon, UserIcon } from './OutputIcon';
import type { PlayOnApi } from './usePlayOn';
import type { RendererInfo } from './remoteTypes';

/** Renderers shown in the picker (admins also get hidden ones from the API; the picker never shows them). */
export function pickerRenderers(renderers: RendererInfo[]): RendererInfo[] {
  return renderers.filter((renderer) => !renderer.hidden);
}

export type OutputPickerVariant = 'desktop' | 'mobile';

/** Mobile rows follow the touch-sized card rows of the mobile sheets. */
const MOBILE_ROW: React.CSSProperties = {
  minHeight: 60,
  marginBottom: 0,
  padding: '10px 12px',
  border: '1px solid var(--divider-subtle)',
  borderRadius: 14,
  background: 'var(--surface-subtle)',
};
const MOBILE_ROW_ACTIVE: React.CSSProperties = {
  borderColor: 'var(--accent)',
  background: 'var(--accent-soft)',
  boxShadow: 'none',
};

function OutputRow({
  icon,
  name,
  detail,
  detailIcon,
  active,
  badge,
  disabled,
  variant,
  onSelect,
}: {
  icon: React.ReactNode;
  name: string;
  detail?: string | null;
  detailIcon?: React.ReactNode;
  active: boolean;
  badge?: React.ReactNode;
  disabled?: boolean;
  variant: OutputPickerVariant;
  onSelect: () => void;
}) {
  const mobile = variant === 'mobile';
  return (
    <button
      type="button"
      role="option"
      aria-selected={active}
      disabled={disabled}
      onClick={onSelect}
      style={{
        ...hybridAudioPanelStyles.listRow,
        ...(active ? hybridAudioPanelStyles.listRowActive : {}),
        ...(mobile ? MOBILE_ROW : {}),
        ...(mobile && active ? MOBILE_ROW_ACTIVE : {}),
        width: '100%',
        gap: mobile ? 12 : 10,
        textAlign: 'left',
        font: 'inherit',
        cursor: disabled ? 'default' : 'pointer',
        ...(disabled ? { opacity: 0.6 } : {}),
      }}
    >
      <span style={{ display: 'flex', flexShrink: 0, color: 'var(--text-muted)' }}>{icon}</span>
      <span style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', gap: 2 }}>
        <span style={{ fontSize: mobile ? 16 : 14, fontWeight: mobile ? 750 : 650, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{name}</span>
        {detail ? (
          <span style={{ display: 'flex', alignItems: 'center', gap: 5, minWidth: 0, fontSize: mobile ? 13 : 12, color: 'var(--text-muted)' }}>
            {detailIcon}
            <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{detail}</span>
          </span>
        ) : null}
      </span>
      {active ? <CheckIcon style={{ flexShrink: 0, color: 'var(--accent)' }} /> : badge}
    </button>
  );
}

function rendererRow(renderer: RendererInfo, playOn: PlayOnApi, variant: OutputPickerVariant, onPicked?: () => void) {
  const iconSize = variant === 'mobile' ? 22 : 20;
  const active = playOn.target.kind === 'renderer' && playOn.target.udn === renderer.udn;
  const session = renderer.session;
  const inUse = Boolean(session?.heldByOther) && !active;
  const pending = playOn.pendingUdn === renderer.udn;
  const detail = inUse
    ? `In use by ${session?.ownerName ?? 'someone else'}${session?.nowPlaying ? ` · ${session.nowPlaying}` : ''}`
    : renderer.model ?? renderer.manufacturer;
  let badge: React.ReactNode = null;
  if (pending) badge = <span style={hybridAudioPanelStyles.badge}>Connecting…</span>;
  else if (!renderer.online) badge = <span style={hybridAudioPanelStyles.badge}>Offline</span>;
  else if (inUse) badge = <span style={{ ...hybridAudioPanelStyles.badge, ...hybridAudioPanelStyles.badgeAccent }}>In use</span>;
  return (
    <OutputRow
      key={renderer.udn}
      icon={<RendererIcon size={iconSize} />}
      name={renderer.name}
      detail={detail}
      detailIcon={inUse ? <UserIcon style={{ flexShrink: 0 }} /> : undefined}
      active={active}
      badge={badge}
      disabled={!renderer.online && !active}
      variant={variant}
      onSelect={() => {
        playOn.selectTarget({ kind: 'renderer', udn: renderer.udn });
        onPicked?.();
      }}
    />
  );
}

export function OutputPickerList({
  playOn,
  variant = 'desktop',
  onPicked,
  onOpenSettings,
}: {
  playOn: PlayOnApi;
  /** `mobile` = touch-sized rows and "This phone". */
  variant?: OutputPickerVariant;
  /** Called after a row is chosen (the popup closes). */
  onPicked?: () => void;
  /** Admin deep link for the empty state. */
  onOpenSettings?: () => void;
}) {
  const listRef = useRef<HTMLDivElement | null>(null);
  const visible = pickerRenderers(playOn.renderers);
  const mobile = variant === 'mobile';

  useEffect(() => {
    const selected = listRef.current?.querySelector<HTMLButtonElement>('button[role="option"][aria-selected="true"]');
    selected?.focus();
  }, []);

  const onKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
    const options = Array.from(listRef.current?.querySelectorAll<HTMLButtonElement>('button[role="option"]:not(:disabled)') ?? []);
    if (!options.length) return;
    event.preventDefault();
    const index = options.indexOf(document.activeElement as HTMLButtonElement);
    const next = event.key === 'Home' ? 0
      : event.key === 'End' ? options.length - 1
        : event.key === 'ArrowDown' ? (index + 1) % options.length
          : (index - 1 + options.length) % options.length;
    options[next].focus();
  };

  return (
    <div
      ref={listRef}
      role="listbox"
      aria-label="Playback devices"
      onKeyDown={onKeyDown}
      style={mobile ? { display: 'grid', gap: 6 } : hybridAudioPanelStyles.list}
    >
      <OutputRow
        icon={mobile ? <PhoneIcon size={22} /> : <BrowserIcon />}
        name={mobile ? 'This phone' : 'This browser'}
        detail={mobile ? 'Plays here' : 'Plays here, with EQ and crossfade'}
        active={playOn.target.kind === 'local'}
        variant={variant}
        onSelect={() => {
          playOn.selectTarget({ kind: 'local' });
          onPicked?.();
        }}
      />
      {visible.map((renderer) => rendererRow(renderer, playOn, variant, onPicked))}
      {playOn.renderersLoaded && visible.length === 0 ? (
        <div style={{ ...hybridAudioPanelStyles.empty, padding: '20px 16px', display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 10 }}>
          <span>No speakers found</span>
          {onOpenSettings ? (
            <button type="button" onClick={onOpenSettings} style={{ ...hybridControlStyles.secondaryButton, minHeight: 32, padding: '6px 10px' }}>
              Speaker settings
            </button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

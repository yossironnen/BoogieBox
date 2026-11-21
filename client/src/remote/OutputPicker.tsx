/**
 * Play On picker rows ("This browser" first, then renderers), shared by the desktop dock popup and the mobile
 * Now Playing sheet. Built from the queue panel's row styles (plan 8.6).
 */

import React, { useEffect, useRef } from 'react';
import { hybridAudioPanelStyles, hybridControlStyles } from '../hybridPreview';
import { BrowserIcon, CheckIcon, PhoneIcon, RendererIcon, UserIcon } from './OutputIcon';
import type { PlayOnApi } from './usePlayOn';
import { DacIcon } from '../native/NativeIcons';
import { deviceRateShort, thisPcLabel } from '../native/nativeFormat';
import { useNativeAudio, useNativeBitPerfect, type NativeAudioState } from '../native/useNativeAudio';
import type { RendererInfo } from './remoteTypes';

/** Renderers shown in the picker (admins also get hidden ones from the API; the picker never shows them). */
export function pickerRenderers(renderers: RendererInfo[]): RendererInfo[] {
  return renderers.filter((renderer) => !renderer.hidden);
}

export type OutputPickerVariant = 'desktop' | 'mobile';

/** Mobile rows follow the touch-sized card rows of the mobile sheets. */
// Fallbacks: the mobile shell has only the base theme tokens (see MobileBottomSheet).
const MOBILE_ROW: React.CSSProperties = {
  minHeight: 60,
  marginBottom: 0,
  padding: '10px 12px',
  border: '1px solid var(--divider-subtle, var(--border))',
  borderRadius: 14,
  background: 'var(--surface-subtle, var(--bg))',
};
const MOBILE_ROW_ACTIVE: React.CSSProperties = {
  borderColor: 'var(--accent)',
  background: 'var(--accent-soft, color-mix(in srgb, var(--accent) 16%, transparent))',
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

/** The desktop app's own engine: "This PC (<machine name>)", with the device list under it when chosen. */
function NativeRows({
  native,
  playOn,
  onPicked,
  onOpenAudioSettings,
}: {
  native: NativeAudioState;
  playOn: PlayOnApi;
  onPicked?: () => void;
  onOpenAudioSettings?: () => void;
}) {
  const bitPerfect = useNativeBitPerfect();
  const active = playOn.target.kind === 'native';
  const deviceName = native.device?.name ?? null;
  const mode = native.settings?.mode === 'shared' ? 'Shared' : bitPerfect ? 'Bit-perfect' : 'Exclusive';
  return (
    <>
      <OutputRow
        icon={<DacIcon />}
        name={thisPcLabel(native.machineName)}
        detail={deviceName ? `${deviceName} · ${mode}` : mode}
        active={active}
        variant="desktop"
        onSelect={() => {
          playOn.selectTarget({ kind: 'native' });
          onPicked?.();
        }}
      />
      {active && native.settings ? (
        <div role="group" aria-label="Output device" style={{ margin: '2px 0 6px 40px', paddingLeft: 10, borderLeft: '1px solid var(--border)', display: 'grid', gap: 2 }}>
          {native.devices.map((device) => {
            const chosen = native.device?.id === device.id;
            const rate = deviceRateShort(device);
            return (
              <button
                key={device.id}
                type="button"
                role="radio"
                aria-checked={chosen}
                onClick={() => {
                  if (native.settings) void native.save({ ...native.settings, deviceId: device.id });
                }}
                style={{
                  display: 'flex', alignItems: 'center', gap: 10, padding: '7px 8px', borderRadius: 10, border: 0,
                  background: chosen ? 'var(--surface-subtle, var(--bg))' : 'transparent', color: 'var(--text)',
                  textAlign: 'left', font: 'inherit', cursor: 'pointer',
                }}
              >
                <span aria-hidden="true" style={{
                  width: 14, height: 14, borderRadius: '50%', flexShrink: 0, boxSizing: 'border-box',
                  border: `2px solid ${chosen ? 'var(--accent)' : 'var(--text-muted)'}`,
                  display: 'flex', alignItems: 'center', justifyContent: 'center',
                }}>
                  {chosen ? <span style={{ width: 6, height: 6, borderRadius: '50%', background: 'var(--accent)' }} /> : null}
                </span>
                <span style={{ flex: 1, minWidth: 0, fontSize: 13, fontWeight: chosen ? 650 : 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{device.name}</span>
                {rate ? <span style={{ fontSize: 11, color: 'var(--text-muted)', whiteSpace: 'nowrap' }}>{rate}</span> : null}
              </button>
            );
          })}
          {onOpenAudioSettings ? (
            <button
              type="button"
              onClick={() => { onOpenAudioSettings(); onPicked?.(); }}
              style={{ justifySelf: 'start', padding: '5px 8px', border: 0, background: 'transparent', color: 'var(--accent)', textDecoration: 'underline', font: 'inherit', fontSize: 12, cursor: 'pointer' }}
            >
              Audio output settings
            </button>
          ) : null}
        </div>
      ) : null}
    </>
  );
}

export function OutputPickerList({
  playOn,
  variant = 'desktop',
  onPicked,
  onOpenSettings,
  onOpenAudioSettings,
}: {
  playOn: PlayOnApi;
  /** `mobile` = touch-sized rows and "This phone". */
  variant?: OutputPickerVariant;
  /** Called after a row is chosen (the popup closes). */
  onPicked?: () => void;
  /** Admin deep link for the empty state. */
  onOpenSettings?: () => void;
  /** Settings › Audio output (desktop app). */
  onOpenAudioSettings?: () => void;
}) {
  const listRef = useRef<HTMLDivElement | null>(null);
  const native = useNativeAudio();
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
      {!mobile && native.available ? (
        <NativeRows native={native} playOn={playOn} onPicked={onPicked} onOpenAudioSettings={onOpenAudioSettings} />
      ) : null}
      {/* Desktop app with the engine: "This PC" replaces "This browser". */}
      {!mobile && native.available ? null : (
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
      )}
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

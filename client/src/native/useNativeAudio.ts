/**
 * The desktop app's Hi-Res output as the UI needs it: whether it is there, the devices (kept current by the
 * engine's device events) and the saved Audio output settings.
 */

import { useCallback, useEffect, useState, useSyncExternalStore } from 'react';
import { nativeAudioAvailable, platform } from '../platform';
import type { NativeAudioSettings, NativeDevice } from './nativeTypes';

export interface NativeAudioState {
  /** The engine answers (desktop app, signed-in server page). */
  available: boolean;
  devices: NativeDevice[];
  settings: NativeAudioSettings | null;
  /** Saves and applies new settings. */
  save: (next: NativeAudioSettings) => Promise<void>;
  /** The device in use: the chosen one, else the Windows default. */
  device: NativeDevice | null;
}

export function activeDevice(devices: NativeDevice[], settings: NativeAudioSettings | null): NativeDevice | null {
  const chosen = settings?.deviceId ? devices.find((d) => d.id === settings.deviceId) : undefined;
  return chosen ?? devices.find((d) => d.isDefault) ?? null;
}

export function useNativeAudio(): NativeAudioState {
  const [available, setAvailable] = useState(false);
  const [devices, setDevices] = useState<NativeDevice[]>([]);
  const [settings, setSettings] = useState<NativeAudioSettings | null>(null);

  useEffect(() => {
    let cancelled = false;
    let unsubscribe: (() => void) | null = null;
    void nativeAudioAvailable().then(async (ok) => {
      if (cancelled || !ok || !platform.audio) return;
      setAvailable(true);
      const audio = platform.audio;
      const [list, saved] = await Promise.all([audio.devices(), audio.settings()]).catch(() => [[], null] as const);
      if (cancelled) return;
      setDevices([...list]);
      setSettings(saved);
      const off = await audio.onEvent((event) => {
        if (event.type === 'deviceChanged') setDevices(event.devices);
      }).catch(() => null);
      if (cancelled) off?.();
      else unsubscribe = off;
    });
    return () => {
      cancelled = true;
      unsubscribe?.();
    };
  }, []);

  const save = useCallback(async (next: NativeAudioSettings) => {
    setSettings(next);
    await platform.audio?.saveSettings(next);
  }, []);

  return { available, devices, settings, save, device: activeDevice(devices, settings) };
}

// The current badge's bit-perfect state, for the picker row (written by the Player's engine hook).
let bitPerfect = false;
const listeners = new Set<() => void>();

export function setNativeBitPerfect(value: boolean): void {
  if (value === bitPerfect) return;
  bitPerfect = value;
  listeners.forEach((l) => l());
}

export function useNativeBitPerfect(): boolean {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => bitPerfect,
    () => false,
  );
}

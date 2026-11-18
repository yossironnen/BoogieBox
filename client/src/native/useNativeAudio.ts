/**
 * The desktop app's Hi-Res output as the UI needs it: whether it is there, the devices (kept current by the
 * engine's device events) and the saved Audio output settings.
 */

import { useCallback, useEffect, useSyncExternalStore } from 'react';
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
  /** This PC's name (the output is called "This PC (<name>)"). */
  machineName: string | null;
}

export function activeDevice(devices: NativeDevice[], settings: NativeAudioSettings | null): NativeDevice | null {
  const chosen = settings?.deviceId ? devices.find((d) => d.id === settings.deviceId) : undefined;
  return chosen ?? devices.find((d) => d.isDefault) ?? null;
}

interface AudioStore {
  available: boolean;
  devices: NativeDevice[];
  settings: NativeAudioSettings | null;
  machineName: string | null;
}

// One copy for the whole page (the Player, the Play on list and Settings all read it), loaded once.
let store: AudioStore = { available: false, devices: [], settings: null, machineName: null };
let started = false;
const storeListeners = new Set<() => void>();

function setStore(change: Partial<AudioStore>): void {
  store = { ...store, ...change };
  storeListeners.forEach((l) => l());
}

function startStore(): void {
  if (started) return;
  started = true;
  void nativeAudioAvailable().then(async (ok) => {
    if (!ok || !platform.audio) return;
    const audio = platform.audio;
    setStore({ available: true });
    const [list, saved] = await Promise.all([audio.devices(), audio.settings()]).catch(() => [[], null] as const);
    setStore({ devices: [...list], settings: saved });
    void audio.machineName().then((name) => setStore({ machineName: name || null })).catch(() => {});
    void audio.onEvent((event) => {
      if (event.type === 'deviceChanged') setStore({ devices: event.devices });
    }).catch(() => {});
  });
}

/** Saves and applies new Audio output settings. */
export async function saveNativeAudioSettings(next: NativeAudioSettings): Promise<void> {
  setStore({ settings: next });
  await platform.audio?.saveSettings(next);
}

/** Test hook: forget the loaded state. */
export function resetNativeAudioStore(): void {
  store = { available: false, devices: [], settings: null, machineName: null };
  started = false;
}

export function useNativeAudio(): NativeAudioState {
  useEffect(() => { startStore(); }, []);
  const current = useSyncExternalStore(
    (l) => {
      storeListeners.add(l);
      return () => storeListeners.delete(l);
    },
    () => store,
    () => store,
  );
  const save = useCallback((next: NativeAudioSettings) => saveNativeAudioSettings(next), []);
  return { ...current, save, device: activeDevice(current.devices, current.settings) };
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

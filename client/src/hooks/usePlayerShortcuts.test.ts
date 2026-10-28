import { renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { shortcutAction, usePlayerShortcuts, type PlayerShortcutHandlers } from './usePlayerShortcuts';

function key(k: string, extra: Partial<KeyboardEventInit> = {}) {
  return { key: k, shiftKey: false, ctrlKey: false, altKey: false, metaKey: false, ...extra };
}

function handlers(): PlayerShortcutHandlers {
  return {
    togglePlay: vi.fn(), seekBack: vi.fn(), seekForward: vi.fn(), previous: vi.fn(), next: vi.fn(),
    volumeUp: vi.fn(), volumeDown: vi.fn(), mute: vi.fn(), search: vi.fn(), help: vi.fn(),
  };
}

function press(target: EventTarget, k: string, init: KeyboardEventInit = {}) {
  const event = new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
}

afterEach(() => { document.body.innerHTML = ''; });

describe('shortcutAction', () => {
  it('maps the bindings', () => {
    expect(shortcutAction(key(' '))).toBe('togglePlay');
    expect(shortcutAction(key('ArrowLeft'))).toBe('seekBack');
    expect(shortcutAction(key('ArrowRight'))).toBe('seekForward');
    expect(shortcutAction(key('ArrowLeft', { shiftKey: true }))).toBe('previous');
    expect(shortcutAction(key('ArrowRight', { shiftKey: true }))).toBe('next');
    expect(shortcutAction(key('ArrowUp', { shiftKey: true }))).toBe('volumeUp');
    expect(shortcutAction(key('ArrowDown', { shiftKey: true }))).toBe('volumeDown');
    expect(shortcutAction(key('m'))).toBe('mute');
    expect(shortcutAction(key('M', { shiftKey: true }))).toBe('mute');
    expect(shortcutAction(key('/'))).toBe('search');
    expect(shortcutAction(key('?', { shiftKey: true }))).toBe('help');
  });

  it('leaves plain up/down and other modifiers alone', () => {
    expect(shortcutAction(key('ArrowUp'))).toBeNull();
    expect(shortcutAction(key('ArrowDown'))).toBeNull();
    expect(shortcutAction(key(' ', { ctrlKey: true }))).toBeNull();
    expect(shortcutAction(key('ArrowLeft', { altKey: true }))).toBeNull();
    expect(shortcutAction(key('m', { metaKey: true }))).toBeNull();
    expect(shortcutAction(key('x'))).toBeNull();
  });
});

describe('usePlayerShortcuts', () => {
  it('runs actions from the page and prevents the default', () => {
    const h = handlers();
    renderHook(() => usePlayerShortcuts(h));
    const event = press(document.body, ' ');
    expect(h.togglePlay).toHaveBeenCalledTimes(1);
    expect(event.defaultPrevented).toBe(true);
    press(document.body, 'ArrowRight', { shiftKey: true });
    expect(h.next).toHaveBeenCalledTimes(1);
    press(document.body, '?', { shiftKey: true });
    expect(h.help).toHaveBeenCalledTimes(1);
  });

  it('ignores keys while typing, in widgets that own them, or with a modal open', () => {
    const h = handlers();
    renderHook(() => usePlayerShortcuts(h));
    document.body.innerHTML = `
      <input id="search" />
      <textarea id="notes"></textarea>
      <div id="editor" contenteditable="true"></div>
      <button id="btn">Go</button>
      <div id="slider" role="slider" tabindex="0"></div>
      <div id="plain" tabindex="0"></div>`;
    for (const id of ['search', 'notes', 'editor']) {
      press(document.getElementById(id)!, ' ');
      press(document.getElementById(id)!, 'ArrowLeft');
      press(document.getElementById(id)!, '/');
    }
    press(document.getElementById('btn')!, ' ');
    press(document.getElementById('slider')!, 'ArrowLeft');
    expect(h.togglePlay).not.toHaveBeenCalled();
    expect(h.seekBack).not.toHaveBeenCalled();
    expect(h.search).not.toHaveBeenCalled();

    // Arrows on a focused button still seek; Space on a plain focusable element still plays.
    press(document.getElementById('btn')!, 'ArrowLeft');
    press(document.getElementById('plain')!, ' ');
    expect(h.seekBack).toHaveBeenCalledTimes(1);
    expect(h.togglePlay).toHaveBeenCalledTimes(1);

    const modal = document.createElement('div');
    modal.setAttribute('role', 'dialog');
    modal.setAttribute('aria-modal', 'true');
    document.body.appendChild(modal);
    press(document.body, ' ');
    press(document.body, 'm');
    expect(h.togglePlay).toHaveBeenCalledTimes(1);
    expect(h.mute).not.toHaveBeenCalled();
  });

  it('skips events another handler already took', () => {
    const h = handlers();
    renderHook(() => usePlayerShortcuts(h));
    const el = document.createElement('div');
    el.addEventListener('keydown', (event) => event.preventDefault());
    document.body.appendChild(el);
    press(el, 'ArrowRight');
    expect(h.seekForward).not.toHaveBeenCalled();
  });

  it('repeats only seeking while a key is held', () => {
    const h = handlers();
    renderHook(() => usePlayerShortcuts(h));
    press(document.body, 'ArrowLeft', { repeat: true });
    press(document.body, 'ArrowLeft', { repeat: true });
    const held = press(document.body, ' ', { repeat: true });
    press(document.body, 'ArrowRight', { shiftKey: true, repeat: true });
    expect(h.seekBack).toHaveBeenCalledTimes(2);
    expect(h.togglePlay).not.toHaveBeenCalled();
    expect(held.defaultPrevented).toBe(true);
    expect(h.next).not.toHaveBeenCalled();
  });

  it('does nothing without handlers', () => {
    const { rerender } = renderHook(({ on }: { on: PlayerShortcutHandlers | null }) => usePlayerShortcuts(on), {
      initialProps: { on: null as PlayerShortcutHandlers | null },
    });
    expect(press(document.body, ' ').defaultPrevented).toBe(false);
    const h = handlers();
    rerender({ on: h });
    press(document.body, ' ');
    expect(h.togglePlay).toHaveBeenCalledTimes(1);
  });
});

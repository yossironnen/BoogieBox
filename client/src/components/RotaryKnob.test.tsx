/**
 * Tests the Rotary Knob (Vintage volume control): keyboard, drag, wheel,
 * double-click mute, clamping and ARIA slider semantics.
 */

import React from 'react';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import RotaryKnob, { KNOB_DRAG_STEP, KNOB_KEY_STEP, KNOB_PAGE_STEP, knobAngle, knobValueForKey } from './RotaryKnob';

describe('knob helpers', () => {
  it('maps 0–1 onto a −135°…+135° sweep and clamps', () => {
    expect(knobAngle(0)).toBe(-135);
    expect(knobAngle(0.5)).toBe(0);
    expect(knobAngle(1)).toBe(135);
    expect(knobAngle(2)).toBe(135);
    expect(knobAngle(-1)).toBe(-135);
  });

  it('steps by key and clamps to 0–1', () => {
    expect(knobValueForKey('ArrowUp', 0.5)).toBeCloseTo(0.5 + KNOB_KEY_STEP);
    expect(knobValueForKey('ArrowRight', 0.5)).toBeCloseTo(0.5 + KNOB_KEY_STEP);
    expect(knobValueForKey('ArrowDown', 0.5)).toBeCloseTo(0.5 - KNOB_KEY_STEP);
    expect(knobValueForKey('ArrowLeft', 0.5)).toBeCloseTo(0.5 - KNOB_KEY_STEP);
    expect(knobValueForKey('PageUp', 0.5)).toBeCloseTo(0.5 + KNOB_PAGE_STEP);
    expect(knobValueForKey('PageDown', 0.5)).toBeCloseTo(0.5 - KNOB_PAGE_STEP);
    expect(knobValueForKey('Home', 0.5)).toBe(0);
    expect(knobValueForKey('End', 0.5)).toBe(1);
    expect(knobValueForKey('ArrowUp', 1)).toBe(1);
    expect(knobValueForKey('ArrowDown', 0)).toBe(0);
    expect(knobValueForKey('Enter', 0.5)).toBeNull();
  });
});

describe('RotaryKnob', () => {
  it('exposes slider semantics with a percentage value', () => {
    render(<RotaryKnob value={0.7} onChange={() => {}} />);
    const knob = screen.getByRole('slider', { name: 'Volume' });
    expect(knob).toHaveAttribute('aria-valuemin', '0');
    expect(knob).toHaveAttribute('aria-valuemax', '100');
    expect(knob).toHaveAttribute('aria-valuenow', '70');
    expect(knob).toHaveAttribute('aria-valuetext', '70%');
    expect(knob).toHaveAttribute('tabindex', '0');
    expect(screen.getByText('VOLUME')).toBeInTheDocument();
  });

  it('reports muted state in aria-valuetext', () => {
    render(<RotaryKnob value={0} muted onChange={() => {}} />);
    expect(screen.getByRole('slider', { name: 'Volume' })).toHaveAttribute('aria-valuetext', 'Muted');
  });

  it('changes value from the keyboard and ignores other keys', () => {
    const onChange = vi.fn();
    render(<RotaryKnob value={0.5} onChange={onChange} />);
    const knob = screen.getByRole('slider', { name: 'Volume' });
    fireEvent.keyDown(knob, { key: 'End' });
    expect(onChange).toHaveBeenLastCalledWith(1);
    fireEvent.keyDown(knob, { key: 'Home' });
    expect(onChange).toHaveBeenLastCalledWith(0);
    onChange.mockClear();
    fireEvent.keyDown(knob, { key: 'Enter' });
    expect(onChange).not.toHaveBeenCalled();
  });

  it('turns with vertical drag (up = louder) and clamps', () => {
    const onChange = vi.fn();
    render(<RotaryKnob value={0.5} onChange={onChange} />);
    const knob = screen.getByRole('slider', { name: 'Volume' });
    fireEvent.pointerDown(knob, { clientY: 100, pointerId: 1 });
    fireEvent.pointerMove(knob, { clientY: 80, pointerId: 1 });
    expect(onChange).toHaveBeenLastCalledWith(0.5 + 20 * KNOB_DRAG_STEP);
    fireEvent.pointerMove(knob, { clientY: 1000, pointerId: 1 });
    expect(onChange).toHaveBeenLastCalledWith(0);
    fireEvent.pointerUp(knob, { pointerId: 1 });
    onChange.mockClear();
    fireEvent.pointerMove(knob, { clientY: 0, pointerId: 1 });
    expect(onChange).not.toHaveBeenCalled();
  });

  it('turns with the mouse wheel', () => {
    const onChange = vi.fn();
    render(<RotaryKnob value={0.5} onChange={onChange} />);
    const knob = screen.getByRole('slider', { name: 'Volume' });
    fireEvent.wheel(knob, { deltaY: -100 });
    expect(onChange).toHaveBeenLastCalledWith(0.5 + KNOB_KEY_STEP);
    fireEvent.wheel(knob, { deltaY: 100 });
    expect(onChange).toHaveBeenLastCalledWith(0.5 - KNOB_KEY_STEP);
  });

  it('toggles mute on double-click', () => {
    const onToggleMute = vi.fn();
    render(<RotaryKnob value={0.5} onChange={() => {}} onToggleMute={onToggleMute} />);
    fireEvent.doubleClick(screen.getByRole('slider', { name: 'Volume' }));
    expect(onToggleMute).toHaveBeenCalledTimes(1);
  });
});

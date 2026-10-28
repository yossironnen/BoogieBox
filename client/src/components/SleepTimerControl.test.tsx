import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { SleepTimerControls } from '../audio/sleepTimer';
import { SleepTimerPanel } from './SleepTimerControl';
import KeyboardShortcutsDialog from './KeyboardShortcutsDialog';

function controls(over: Partial<SleepTimerControls> = {}): SleepTimerControls {
  return { timer: null, local: true, start: vi.fn(), cancel: vi.fn(), ...over };
}

describe('SleepTimerPanel', () => {
  it('starts presets and end-of modes, then closes', () => {
    const c = controls();
    const onDone = vi.fn();
    render(<SleepTimerPanel controls={c} onDone={onDone} />);
    for (const label of ['15 min', '30 min', '45 min', '60 min', '90 min']) {
      expect(screen.getByRole('button', { name: label })).toBeInTheDocument();
    }
    fireEvent.click(screen.getByRole('button', { name: '30 min' }));
    expect(c.start).toHaveBeenLastCalledWith('minutes', 30);
    fireEvent.click(screen.getByRole('button', { name: 'End of track' }));
    expect(c.start).toHaveBeenLastCalledWith('endOfTrack', undefined);
    fireEvent.click(screen.getByRole('button', { name: 'End of album' }));
    expect(c.start).toHaveBeenLastCalledWith('endOfAlbum', undefined);
    expect(onDone).toHaveBeenCalledTimes(3);
    expect(screen.getByText('Reloading the page clears the timer.')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Cancel timer' })).not.toBeInTheDocument();
  });

  it('validates custom minutes', () => {
    const c = controls();
    render(<SleepTimerPanel controls={c} />);
    fireEvent.click(screen.getByRole('button', { name: 'Custom…' }));
    const input = screen.getByLabelText('Minutes');
    const startButton = screen.getByRole('button', { name: 'Start' });
    for (const bad of ['0', '601', '2.5', '']) {
      fireEvent.change(input, { target: { value: bad } });
      expect(startButton).toBeDisabled();
    }
    fireEvent.change(input, { target: { value: '120' } });
    fireEvent.click(startButton);
    expect(c.start).toHaveBeenCalledWith('minutes', 120);
  });

  it('shows the running timer with cancel, and no reload note for a speaker', () => {
    const c = controls({ timer: { mode: 'endOfAlbum', deadlineMs: null }, local: false });
    render(<SleepTimerPanel controls={c} />);
    expect(screen.getByRole('status')).toHaveTextContent('Sleep timer: end of album');
    fireEvent.click(screen.getByRole('button', { name: 'Cancel timer' }));
    expect(c.cancel).toHaveBeenCalled();
    expect(screen.queryByText('Reloading the page clears the timer.')).not.toBeInTheDocument();
  });
});

describe('KeyboardShortcutsDialog', () => {
  it('lists the bindings and closes on Close or Escape', () => {
    const onClose = vi.fn();
    render(<KeyboardShortcutsDialog onClose={onClose} />);
    const dialog = screen.getByRole('dialog', { name: 'Keyboard shortcuts' });
    expect(dialog).toHaveAttribute('aria-modal', 'true');
    for (const label of ['Play / pause', 'Seek back / forward 10 s', 'Previous / next track', 'Volume up / down', 'Mute', 'Search', 'Show shortcuts']) {
      expect(screen.getByText(label)).toBeInTheDocument();
    }
    expect(screen.getByRole('button', { name: 'Close' })).toHaveFocus();
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});

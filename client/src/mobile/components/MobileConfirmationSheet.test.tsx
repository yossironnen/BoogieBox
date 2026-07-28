import React, { useState } from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import MobileConfirmationSheet from './MobileConfirmationSheet';

function Harness({
  onConfirm,
}: {
  onConfirm: () => Promise<unknown> | unknown;
}) {
  const [open, setOpen] = useState(true);
  return (
    <>
      <button type="button">Return target</button>
      <MobileConfirmationSheet
        open={open}
        title="Remove this track?"
        description="This removes the track from the playlist."
        itemLabel="A Song"
        confirmLabel="Remove track"
        busyLabel="Removing…"
        onClose={() => setOpen(false)}
        onConfirm={onConfirm}
      />
    </>
  );
}

describe('MobileConfirmationSheet', () => {
  it('uses alert-dialog semantics and keeps the safe action touch-sized', async () => {
    const onConfirm = vi.fn();
    render(<Harness onConfirm={onConfirm} />);

    const dialog = screen.getByRole('alertdialog', { name: 'Remove this track?' });
    expect(dialog).toBeInTheDocument();
    expect(screen.getByText('A Song')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Keep it' })).toHaveStyle({ minHeight: '52px' });
    await waitFor(() => expect(screen.getByRole('button', { name: 'Cancel Remove this track?' })).toHaveFocus());

    fireEvent.click(screen.getByRole('button', { name: 'Keep it' }));
    expect(onConfirm).not.toHaveBeenCalled();
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
  });

  it('blocks dismissal while busy and closes after confirmation succeeds', async () => {
    let resolveConfirm: (() => void) | undefined;
    const onConfirm = vi.fn(() => new Promise<void>((resolve) => {
      resolveConfirm = resolve;
    }));
    render(<Harness onConfirm={onConfirm} />);

    fireEvent.click(screen.getByRole('button', { name: 'Remove track' }));
    expect(await screen.findByRole('button', { name: 'Removing…' })).toBeDisabled();
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(screen.getByRole('alertdialog')).toBeInTheDocument();

    resolveConfirm?.();
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument());
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it('keeps the confirmation open and reports rejected actions', async () => {
    render(<Harness onConfirm={() => Promise.reject(new Error('Server refused removal'))} />);

    fireEvent.click(screen.getByRole('button', { name: 'Remove track' }));
    expect(await screen.findByText('Server refused removal')).toBeInTheDocument();
    expect(screen.getByRole('alertdialog')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Keep it' }));
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
  });
});

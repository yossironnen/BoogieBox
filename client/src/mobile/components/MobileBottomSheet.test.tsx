import React, { useState } from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import MobileBottomSheet from './MobileBottomSheet';

function Harness({ onClose = vi.fn() }: { onClose?: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>Open sheet</button>
      {open ? (
        <MobileBottomSheet
          title="Sample Sheet"
          onClose={() => {
            onClose();
            setOpen(false);
          }}
        >
          <div>Sheet content</div>
        </MobileBottomSheet>
      ) : null}
    </>
  );
}

describe('MobileBottomSheet', () => {
  it('uses the shared semantic frame, Escape dismissal, scroll lock, and focus return', async () => {
    const onClose = vi.fn();
    render(<Harness onClose={onClose} />);
    const opener = screen.getByRole('button', { name: 'Open sheet' });
    opener.focus();
    fireEvent.click(opener);

    expect(screen.getByRole('dialog', { name: 'Sample Sheet' })).toHaveStyle({ maxHeight: '84dvh' });
    expect(screen.getByRole('button', { name: 'Close' })).toHaveStyle({ minWidth: '44px', height: '44px' });
    expect(document.body.style.overflow).toBe('hidden');

    fireEvent.keyDown(window, { key: 'Escape' });

    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Sample Sheet' })).not.toBeInTheDocument());
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(document.body.style.overflow).toBe('');
    expect(opener).toHaveFocus();
  });

  it('dismisses from the semantic scrim', () => {
    const onClose = vi.fn();
    render(<Harness onClose={onClose} />);
    fireEvent.click(screen.getByRole('button', { name: 'Open sheet' }));
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss Sample Sheet' }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import ConfirmModal from './ConfirmModal';

describe('ConfirmModal', () => {
  it('renders the title, message, and default button labels', () => {
    render(
      <ConfirmModal title="Remove Library" message="Remove this library and all its scanned data?" onConfirm={vi.fn()} onCancel={vi.fn()} />,
    );
    expect(screen.getByText('Remove Library')).toBeInTheDocument();
    expect(screen.getByText('Remove this library and all its scanned data?')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Confirm' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeInTheDocument();
  });

  it('calls onConfirm when the confirm button is clicked', () => {
    const onConfirm = vi.fn();
    render(<ConfirmModal title="Delete user" message="Delete this user?" confirmLabel="Delete" onConfirm={onConfirm} onCancel={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it('calls onCancel when the cancel button, the backdrop, or Escape fires', () => {
    const onCancel = vi.fn();
    const { unmount } = render(<ConfirmModal title="Remove folder" message="Remove this folder?" onConfirm={vi.fn()} onCancel={onCancel} />);
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    unmount();

    const onCancel2 = vi.fn();
    render(<ConfirmModal title="Remove folder" message="Remove this folder?" onConfirm={vi.fn()} onCancel={onCancel2} />);
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onCancel2).toHaveBeenCalledTimes(1);
  });

  it('closes on a backdrop click but not on a click inside the card', () => {
    const onCancel = vi.fn();
    render(<ConfirmModal title="Remove folder" message="Remove this folder?" onConfirm={vi.fn()} onCancel={onCancel} />);
    fireEvent.click(screen.getByText('Remove folder'));
    expect(onCancel).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('presentation'));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it('renders a single acknowledgement button when cancelLabel is null', () => {
    render(<ConfirmModal title="No results" message="No radio tracks found." cancelLabel={null} confirmLabel="OK" onConfirm={vi.fn()} onCancel={vi.fn()} />);
    expect(screen.queryByRole('button', { name: 'Cancel' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'OK' })).toBeInTheDocument();
  });

  it('disables both buttons and ignores Escape/backdrop while busy', () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(
      <ConfirmModal
        title="Clear cache" message="Clear the cache?" busy busyLabel="Clearing…"
        onConfirm={onConfirm} onCancel={onCancel}
      />,
    );
    const confirmBtn = screen.getByRole('button', { name: 'Clearing…' });
    const cancelBtn = screen.getByRole('button', { name: 'Cancel' });
    expect(confirmBtn).toBeDisabled();
    expect(cancelBtn).toBeDisabled();

    fireEvent.keyDown(window, { key: 'Escape' });
    fireEvent.click(screen.getByRole('presentation'));
    expect(onCancel).not.toHaveBeenCalled();
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it('colors the eyebrow and confirm button for the danger tone', () => {
    render(<ConfirmModal title="Delete user" message="This cannot be undone." tone="danger" confirmLabel="Delete" onConfirm={vi.fn()} onCancel={vi.fn()} />);
    expect(screen.getByText('Confirm — this cannot be undone')).toBeInTheDocument();
    const confirmBtn = screen.getByRole('button', { name: 'Delete' });
    expect(confirmBtn).toHaveStyle({ background: 'var(--danger, #dc2626)' });
  });
});

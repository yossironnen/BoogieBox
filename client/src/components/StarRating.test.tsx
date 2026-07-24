import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import StarRating from './StarRating';

describe('StarRating', () => {
  it('commits half-star clicks, clears repeated values, and previews hover', () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <StarRating value={2.5} onChange={onChange} ariaLabel="Track rating" showValue />,
    );
    const slider = screen.getByRole('slider', { name: 'Track rating' });
    expect(slider).toHaveAttribute('aria-valuetext', '2.5 / 5');
    const same = screen.getByRole('button', { name: 'Set rating to 2.5 stars' });
    fireEvent.mouseEnter(same);
    fireEvent.click(same);
    expect(onChange).toHaveBeenCalledWith(null);
    fireEvent.mouseLeave(slider);
    fireEvent.focus(slider);
    expect(slider.style.outline).toContain('2px solid');
    fireEvent.blur(slider);

    rerender(<StarRating value={null} onChange={onChange} ariaLabel="Track rating" size="compact" subdued showValue />);
    fireEvent.click(screen.getByRole('button', { name: 'Set rating to 5 stars' }));
    expect(onChange).toHaveBeenCalledWith(5);
    expect(slider).toHaveAttribute('aria-valuetext', 'Unrated');
  });

  it('supports all keyboard rating controls and bounds', () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <StarRating value={null} onChange={onChange} ariaLabel="Album rating" size="hero" />,
    );
    let slider = screen.getByRole('slider');
    fireEvent.keyDown(slider, { key: 'ArrowRight' });
    fireEvent.keyDown(slider, { key: 'ArrowLeft' });
    fireEvent.keyDown(slider, { key: 'Home' });
    fireEvent.keyDown(slider, { key: 'End' });
    fireEvent.keyDown(slider, { key: 'Delete' });
    expect(onChange.mock.calls.map(([value]) => value)).toEqual([0.5, null, 0.5, 5, null]);

    rerender(<StarRating value={5} onChange={onChange} ariaLabel="Album rating" />);
    slider = screen.getByRole('slider');
    fireEvent.keyDown(slider, { key: 'ArrowUp' });
    fireEvent.keyDown(slider, { key: 'ArrowDown' });
    fireEvent.keyDown(slider, { key: 'Backspace' });
    fireEvent.keyDown(slider, { key: '0' });
    expect(onChange).toHaveBeenCalledWith(5);
    expect(onChange).toHaveBeenCalledWith(4.5);
  });

  it('renders a read-only unrated indicator', () => {
    render(<StarRating value={undefined} ariaLabel="Read only rating" showValue />);
    expect(screen.getByRole('img', { name: 'Read only rating' })).toHaveTextContent('-');
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });
});

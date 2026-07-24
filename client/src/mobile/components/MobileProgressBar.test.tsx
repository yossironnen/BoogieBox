import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import MobileProgressBar from './MobileProgressBar';

describe('MobileProgressBar', () => {
  it.each([
    { value: undefined, max: undefined },
    { value: 0, max: 10 },
    { value: 5, max: 0 },
    { value: -5, max: 10 },
  ])('hides zero or invalid progress', ({ value, max }) => {
    const { container } = render(<MobileProgressBar value={value} max={max} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('calculates, clamps, and sizes visible progress', () => {
    const { container, rerender } = render(<MobileProgressBar value={5} max={10} height={8} />);
    expect(container.firstChild).toHaveStyle({ height: '8px' });
    expect(container.querySelector('span span')).toHaveStyle({ width: '50%' });
    rerender(<MobileProgressBar value={20} max={10} />);
    expect(container.querySelector('span span')).toHaveStyle({ width: '100%' });
  });
});

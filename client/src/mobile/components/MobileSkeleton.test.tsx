import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import MobileSkeleton from './MobileSkeleton';

describe('MobileSkeleton', () => {
  it('renders the default row count and supports empty output', () => {
    const view = render(<MobileSkeleton />);
    expect(view.container.querySelectorAll('span')).toHaveLength(4);
    view.rerender(<MobileSkeleton count={0} />);
    expect(view.container.querySelectorAll('span')).toHaveLength(0);
  });

  it('renders album and poster grid variants', () => {
    const view = render(<MobileSkeleton variant="album" count={2} />);
    expect(view.container.querySelectorAll('span')).toHaveLength(2);
    expect(view.container.querySelector('span')?.getAttribute('style')).toContain('aspect-ratio: 1 / 1');
    view.rerender(<MobileSkeleton variant="poster" count={1} />);
    expect(view.container.querySelector('span')?.getAttribute('style')).toContain('aspect-ratio: 2 / 3');
  });
});

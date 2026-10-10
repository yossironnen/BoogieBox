/**
 * Tests the shared browse CommandBar.
 */

import React from 'react';
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import CommandBar from './CommandBar';

describe('CommandBar', () => {
  it('draws a decorative, non-interactive watermark behind the controls', () => {
    render(<CommandBar watermark="Browse"><button type="button">Tab</button></CommandBar>);
    const watermark = screen.getByTestId('command-bar-watermark');
    expect(watermark).toHaveTextContent('Browse');
    expect(watermark.parentElement).toHaveAttribute('aria-hidden', 'true');
    expect(watermark.parentElement).toHaveStyle({ pointerEvents: 'none', overflow: 'hidden' });
    expect(watermark).toHaveStyle({ opacity: '0.12', fontSize: '100px' });
    expect(screen.getByRole('button', { name: 'Tab' })).toBeInTheDocument();
  });

  it('renders the actions zone only when actions are given', () => {
    const { container, rerender } = render(<CommandBar watermark="Search"><span>left</span></CommandBar>);
    const bar = container.querySelector('[data-command-bar="search"]') as HTMLElement;
    expect(bar.children).toHaveLength(2);

    rerender(<CommandBar watermark="Search" actions={<button type="button">Act</button>}><span>left</span></CommandBar>);
    expect(bar.children).toHaveLength(3);
    expect(screen.getByRole('button', { name: 'Act' })).toBeInTheDocument();
  });

  it('keeps the bar itself overflow-visible so anchored popovers are not clipped', () => {
    const { container } = render(<CommandBar watermark="Browse"><span>left</span></CommandBar>);
    const bar = container.querySelector('[data-command-bar="browse"]') as HTMLElement;
    expect(bar.style.overflow).toBe('');
    expect(bar).toHaveStyle({ position: 'relative', flexWrap: 'nowrap' });
    // The bar is its own stacking context; it must sit above the grid/table so
    // the Refine, sort and genre popovers render over the content.
    expect(bar).toHaveStyle({ zIndex: '30' });
  });
});

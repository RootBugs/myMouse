import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import DeviceLayout from './DeviceLayout';
import type { DeviceLayoutData } from '../types';

const topology: DeviceLayoutData = {
  nodes: [
    { deviceId: 'pc-a', monitorIndex: 0, width: 1920, height: 1080 },
    { deviceId: 'pc-b', monitorIndex: 1, width: 1280, height: 720 },
  ],
  edges: [{ from: 'pc-a', to: 'pc-b', side: 'right' }],
};

describe('DeviceLayout', () => {
  it('renders backend-provided nodes and edges', () => {
    const { container } = render(<DeviceLayout layout={topology} />);

    expect(screen.getAllByRole('group')).toHaveLength(2);
    expect(screen.getByText('pc-a : 0')).toBeInTheDocument();
    expect(screen.getByText('pc-b : 1')).toBeInTheDocument();
    expect(container.querySelectorAll('[data-testid="layout-edge"]')).toHaveLength(1);
  });

  it('shows an explicit empty state when no topology is configured', () => {
    render(<DeviceLayout layout={{ nodes: [], edges: [] }} />);
    expect(screen.getByText('No layout configured yet.')).toBeInTheDocument();
  });
});

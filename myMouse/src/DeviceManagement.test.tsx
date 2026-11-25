import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import DeviceManagement from './components/DeviceManagement';

const peer = {
  id: 'pc-b',
  name: 'PC-B',
  address: '192.168.1.20',
  port: 24800,
  fingerprint: 'a'.repeat(64),
};

function mockBridge(overrides: Record<string, unknown> = {}) {
  (window as any).mymouse = {
    pairingUri: async () => 'mymouse://pair?code=123456&id=pc-a',
    approvePairing: async () => ({ ok: true }),
    discoveredPeers: async () => ({ ok: true, peers: [] }),
    onNotification: () => () => undefined,
    ...overrides,
  };
}

describe('DeviceManagement discovered peers', () => {
  it('renders discovered peers from the backend', async () => {
    mockBridge({ discoveredPeers: vi.fn(async () => ({ ok: true, peers: [peer] })) });

    render(<DeviceManagement devices={[]} />);

    expect(await screen.findByText(peer.name)).toBeInTheDocument();
    expect(screen.getByText(peer.id)).toBeInTheDocument();
    expect(screen.getByText(`${peer.address}:${peer.port}`)).toBeInTheDocument();
    expect(screen.getByText(`Fingerprint: ${peer.fingerprint.slice(0, 12)}…`)).toBeInTheDocument();
  });

  it('refreshes discovered peers when a discoveredPeer notification arrives', async () => {
    let listener: ((message: Record<string, unknown>) => void) | undefined;
    const discoveredPeers = vi.fn()
      .mockResolvedValueOnce({ ok: true, peers: [] })
      .mockResolvedValue({ ok: true, peers: [peer] });
    mockBridge({
      discoveredPeers,
      onNotification: (cb: (message: Record<string, unknown>) => void) => {
        listener = cb;
        return () => { listener = undefined; };
      },
    });

    render(<DeviceManagement devices={[]} />);
    await waitFor(() => expect(discoveredPeers).toHaveBeenCalledTimes(1));
    expect(screen.queryByText(peer.name)).not.toBeInTheDocument();

    act(() => listener?.({ notification: 'discoveredPeer', ...peer }));
    expect(await screen.findByText(peer.name)).toBeInTheDocument();
    await waitFor(() => expect(discoveredPeers).toHaveBeenCalledTimes(2));
  });

  it('pairs the selected peer and refreshes the list', async () => {
    const discoveredPeers = vi.fn(async () => ({ ok: true, peers: [peer] }));
    const pairDiscovered = vi.fn(async () => ({ ok: true }));
    mockBridge({ discoveredPeers, pairDiscovered });

    render(<DeviceManagement devices={[]} />);
    expect(await screen.findByText(peer.name)).toBeInTheDocument();
    expect(discoveredPeers).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole('button', { name: 'Pair' }));
    await waitFor(() => expect(pairDiscovered).toHaveBeenCalledWith(peer.id));
    expect(await screen.findByText(`Paired with ${peer.name}.`)).toBeInTheDocument();
    await waitFor(() => expect(discoveredPeers).toHaveBeenCalledTimes(2));
  });

  it('shows the backend error when pairing fails', async () => {
    mockBridge({
      discoveredPeers: async () => ({ ok: true, peers: [peer] }),
      pairDiscovered: async () => ({ ok: false, error: 'Approval required on both peers.' }),
    });

    render(<DeviceManagement devices={[]} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Pair' }));

    expect(await screen.findByText('Approval required on both peers.')).toBeInTheDocument();
  });
});

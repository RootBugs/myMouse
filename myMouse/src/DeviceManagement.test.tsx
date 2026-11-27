import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import DeviceManagement from './components/DeviceManagement';

const fingerprint = 'a'.repeat(64);
const peer = {
  id: 'pc-b',
  name: 'PC-B',
  address: '192.168.1.20',
  port: 24800,
  fingerprint,
};

const trustedDevice = {
  id: 'pc-b',
  name: 'PC-B',
  capabilities: { input: true, clipboard: false, files: true, screen: false },
};

function mockBridge(overrides: Record<string, unknown> = {}) {
  (window as any).mymouse = {
    pairingUri: async () => 'mymouse://pair?code=123456&id=pc-a',
    approvePairing: async () => ({ ok: true }),
    discoveredPeers: async () => ({ ok: true, peers: [] }),
    onNotification: () => () => undefined,
    setDeviceCapabilities: async () => ({ ok: true }),
    revokeDevice: async () => ({ ok: true }),
    ...overrides,
  };
}

async function confirmPeer(name = peer.name, value = fingerprint) {
  fireEvent.change(screen.getByLabelText(`Confirm fingerprint for ${name}`), {
    target: { value },
  });
  fireEvent.click(screen.getByLabelText(`Verified ${name} fingerprint`));
}

describe('DeviceManagement discovered peers', () => {
  it('renders discovered peers from the backend', async () => {
    mockBridge({ discoveredPeers: vi.fn(async () => ({ ok: true, peers: [peer] })) });

    render(<DeviceManagement devices={[]} onRefresh={vi.fn(async () => {})} />);

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

    render(<DeviceManagement devices={[]} onRefresh={vi.fn(async () => {})} />);
    await waitFor(() => expect(discoveredPeers).toHaveBeenCalledTimes(1));
    expect(screen.queryByText(peer.name)).not.toBeInTheDocument();

    act(() => listener?.({ notification: 'discoveredPeer', ...peer }));
    expect(await screen.findByText(peer.name)).toBeInTheDocument();
    await waitFor(() => expect(discoveredPeers).toHaveBeenCalledTimes(2));
  });

  it('requires an exact verified fingerprint before pairing a discovered peer', async () => {
    const pairDiscovered = vi.fn(async () => ({ ok: true }));
    mockBridge({
      discoveredPeers: async () => ({ ok: true, peers: [peer] }),
      pairDiscovered,
    });

    render(<DeviceManagement devices={[]} onRefresh={vi.fn(async () => {})} />);
    await screen.findByText(peer.name);
    const pairButton = screen.getByRole('button', { name: 'Pair' });
    expect(pairButton).toBeDisabled();

    await confirmPeer(peer.name, 'b'.repeat(64));
    expect(pairButton).toBeDisabled();

    fireEvent.change(screen.getByLabelText(`Confirm fingerprint for ${peer.name}`), {
      target: { value: fingerprint },
    });
    expect(pairButton).toBeEnabled();

    fireEvent.click(pairButton);
    await waitFor(() => expect(pairDiscovered).toHaveBeenCalledWith(peer.id, fingerprint, true));
    expect(await screen.findByText(`Paired with ${peer.name}.`)).toBeInTheDocument();
  });

  it('shows the backend error when pairing fails', async () => {
    mockBridge({
      discoveredPeers: async () => ({ ok: true, peers: [peer] }),
      pairDiscovered: async () => ({ ok: false, error: 'Fingerprint does not match the discovered peer' }),
    });

    render(<DeviceManagement devices={[]} onRefresh={vi.fn(async () => {})} />);
    await screen.findByText(peer.name);
    await confirmPeer();
    fireEvent.click(screen.getByRole('button', { name: 'Pair' }));

    expect(await screen.findByText('Fingerprint does not match the discovered peer')).toBeInTheDocument();
  });

  it('binds the QR pairing session to the verified joining peer', async () => {
    const pairingUri = vi.fn(async () => 'mymouse://pair?code=123456&id=pc-a');
    mockBridge({ pairingUri });

    render(<DeviceManagement devices={[]} onRefresh={vi.fn(async () => {})} />);
    expect(pairingUri).not.toHaveBeenCalled();

    fireEvent.change(screen.getByLabelText('Pairing peer ID'), { target: { value: peer.id } });
    fireEvent.change(screen.getByLabelText('Pairing peer fingerprint'), {
      target: { value: fingerprint },
    });
    fireEvent.click(screen.getByLabelText('Verify joining peer fingerprint'));

    await waitFor(() => expect(pairingUri).toHaveBeenCalledWith(peer.id, fingerprint));
  });

  it('requires peer ID, exact fingerprint and verification before approving a pairing code', async () => {
    const approvePairing = vi.fn(async () => ({ ok: true }));
    mockBridge({ approvePairing });

    render(<DeviceManagement devices={[]} onRefresh={vi.fn(async () => {})} />);
    const approveButton = screen.getByRole('button', { name: 'Approve' });
    expect(approveButton).toBeDisabled();

    fireEvent.change(screen.getByLabelText('Pairing code'), { target: { value: '123456' } });
    fireEvent.change(screen.getByLabelText('Pairing peer ID'), { target: { value: peer.id } });
    fireEvent.change(screen.getByLabelText('Pairing peer fingerprint'), { target: { value: 'not-a-fingerprint' } });
    fireEvent.click(screen.getByLabelText('Verify joining peer fingerprint'));
    expect(approveButton).toBeDisabled();

    const joiningFingerprint = 'b'.repeat(64);
    fireEvent.change(screen.getByLabelText('Pairing peer fingerprint'), { target: { value: joiningFingerprint } });
    expect(approveButton).toBeEnabled();
    fireEvent.click(approveButton);
    await waitFor(() => expect(approvePairing).toHaveBeenCalledWith('123456', peer.id, joiningFingerprint, true));
  });
});

describe('DeviceManagement trusted devices', () => {
  it('sends all capabilities when one toggle changes', async () => {
    const setDeviceCapabilities = vi.fn(async () => ({ ok: true }));
    mockBridge({ setDeviceCapabilities });

    render(<DeviceManagement devices={[trustedDevice]} onRefresh={vi.fn(async () => {})} />);
    expect(screen.getByLabelText('Input for PC-B')).toBeChecked();
    expect(screen.getByLabelText('Clipboard for PC-B')).not.toBeChecked();

    fireEvent.click(screen.getByLabelText('Clipboard for PC-B'));
    await waitFor(() => expect(setDeviceCapabilities).toHaveBeenCalledWith(
      trustedDevice.id,
      true,
      true,
      true,
      false,
    ));
  });

  it('revokes a trusted device and refreshes the list', async () => {
    const revokeDevice = vi.fn(async () => ({ ok: true }));
    const onRefresh = vi.fn(async () => {});
    mockBridge({ revokeDevice });

    render(<DeviceManagement devices={[trustedDevice]} onRefresh={onRefresh} />);
    fireEvent.click(screen.getByRole('button', { name: 'Revoke PC-B' }));

    await waitFor(() => expect(revokeDevice).toHaveBeenCalledWith(trustedDevice.id));
    await waitFor(() => expect(onRefresh).toHaveBeenCalled());
  });
});

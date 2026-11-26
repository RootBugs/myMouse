import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import App from './App';

describe('App', () => {
  it('shows connected state and device list', async () => {
    (window as any).mymouse = {
      status: async () => ({ ok: true, connected: true }),
      devices: async () => [{ name: 'PC-B' }],
      pairingUri: async () => 'mymouse://pair?code=123456&id=pc-a',
      approvePairing: async () => ({ ok: true }),
      setLayout: async () => ({ ok: true }),
      getLayout: async () => ({ ok: true, layout: { nodes: [], edges: [] } }),
      getSettings: async () => ({
        ok: true,
        settings: {
          clipboard: { enabled: true, mode: 'global' },
          file_transfer: { auto_accept: false, save_dir: '', max_transfer_bytes: 100 * 1024 * 1024 },
          auto_reconnect: true,
          launch_at_start: false,
          quick_drop_enabled: true,
        },
      }),
      listTransfers: async () => ({ ok: true, transfers: [] }),
    };

    render(<App />);
    expect(await screen.findByText('Connected')).toBeInTheDocument();
    expect(screen.getByText('PC-B')).toBeInTheDocument();
  });

  it('refreshes state when the backend sends a notification', async () => {
    let listener: ((message: Record<string, unknown>) => void) | undefined;
    const listTransfers = vi.fn(async () => ({ ok: true, transfers: [] }));
    (window as any).mymouse = {
      status: async () => ({ ok: true, connected: false }),
      devices: async () => [],
      pairingUri: async () => 'mymouse://pair?code=123456&id=pc-a',
      approvePairing: async () => ({ ok: true }),
      setLayout: async () => ({ ok: true }),
      getLayout: async () => ({ ok: true, layout: { nodes: [], edges: [] } }),
      getSettings: async () => ({
        ok: true,
        settings: {
          clipboard: { enabled: true, mode: 'global' },
          file_transfer: { auto_accept: false, save_dir: '', max_transfer_bytes: 100 * 1024 * 1024 },
          auto_reconnect: true,
          launch_at_start: false,
          quick_drop_enabled: true,
        },
      }),
      listTransfers,
      onNotification: (cb: (message: Record<string, unknown>) => void) => {
        listener = cb;
        return () => { listener = undefined; };
      },
    };

    render(<App />);
    expect(await screen.findByText('Disconnected')).toBeInTheDocument();
    expect(listTransfers).toHaveBeenCalledTimes(1);

    act(() => listener?.({ notification: 'transferUpdate' }));
    await waitFor(() => expect(listTransfers).toHaveBeenCalledTimes(2));
  });

  it('sends clipboard text to the connected peer', async () => {
    const sendClipboard = vi.fn(async () => ({ ok: true }));
    (window as any).mymouse = {
      status: async () => ({ ok: true, connected: true }),
      devices: async () => [{ name: 'PC-B', id: 'pc-b' }],
      pairingUri: async () => 'mymouse://pair?code=123456&id=pc-a',
      approvePairing: async () => ({ ok: true }),
      setLayout: async () => ({ ok: true }),
      getLayout: async () => ({ ok: true, layout: { nodes: [], edges: [] } }),
      getSettings: async () => ({
        ok: true,
        settings: {
          clipboard: { enabled: true, mode: 'global' },
          file_transfer: { auto_accept: false, save_dir: '', max_transfer_bytes: 100 * 1024 * 1024 },
          auto_reconnect: true,
          launch_at_start: false,
          quick_drop_enabled: true,
        },
      }),
      listTransfers: async () => ({ ok: true, transfers: [] }),
      sendClipboard,
    };

    render(<App />);
    expect(await screen.findByText('Connected')).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('Clipboard text to send'), { target: { value: 'hello peer' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));
    await waitFor(() => expect(sendClipboard).toHaveBeenCalledWith('hello peer'));
    expect(await screen.findByText('Sent to peer')).toBeInTheDocument();
  });

  it('rejects an invalid fingerprint and connects with a valid pinned peer', async () => {
    const connectPeer = vi.fn(async () => ({ ok: true }));
    (window as any).mymouse = {
      status: async () => ({ ok: true, connected: false }),
      devices: async () => [],
      pairingUri: async () => 'mymouse://pair?code=123456&id=pc-a',
      approvePairing: async () => ({ ok: true }),
      setLayout: async () => ({ ok: true }),
      getLayout: async () => ({ ok: true, layout: { nodes: [], edges: [] } }),
      getSettings: async () => ({
        ok: true,
        settings: {
          clipboard: { enabled: true, mode: 'global' },
          file_transfer: { auto_accept: false, save_dir: '', max_transfer_bytes: 100 * 1024 * 1024 },
          auto_reconnect: true,
          launch_at_start: false,
          quick_drop_enabled: true,
        },
      }),
      listTransfers: async () => ({ ok: true, transfers: [] }),
      connectPeer,
    };

    render(<App />);
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }));

    fireEvent.change(screen.getByLabelText('Peer address'), { target: { value: '192.168.1.20:24800' } });
    fireEvent.change(screen.getByLabelText('Peer device ID'), { target: { value: 'pc-b' } });
    fireEvent.change(screen.getByLabelText('Pinned fingerprint'), { target: { value: 'not-a-fingerprint' } });
    fireEvent.click(screen.getByRole('button', { name: 'Connect with pinned fingerprint' }));
    expect(connectPeer).not.toHaveBeenCalled();
    expect(await screen.findByText('Fingerprint must be 64 hex characters.')).toBeInTheDocument();

    const fingerprint = 'a'.repeat(64);
    fireEvent.change(screen.getByLabelText('Pinned fingerprint'), { target: { value: fingerprint } });
    fireEvent.click(screen.getByRole('button', { name: 'Connect with pinned fingerprint' }));
    await waitFor(() => expect(connectPeer).toHaveBeenCalledWith('192.168.1.20:24800', 'pc-b', fingerprint));
    expect(await screen.findByText('Connected to pinned peer.')).toBeInTheDocument();
  });

  it('shows the local fingerprint for pairing', async () => {
    const fingerprint = 'b'.repeat(64);
    (window as any).mymouse = {
      status: async () => ({ ok: true, connected: false }),
      devices: async () => [],
      pairingUri: async () => 'mymouse://pair?code=123456&id=pc-a',
      approvePairing: async () => ({ ok: true }),
      setLayout: async () => ({ ok: true }),
      getLayout: async () => ({ ok: true, layout: { nodes: [], edges: [] } }),
      getSettings: async () => ({
        ok: true,
        settings: {
          clipboard: { enabled: true, mode: 'global' },
          file_transfer: { auto_accept: false, save_dir: '', max_transfer_bytes: 100 * 1024 * 1024 },
          auto_reconnect: true,
          launch_at_start: false,
          quick_drop_enabled: true,
        },
      }),
      listTransfers: async () => ({ ok: true, transfers: [] }),
      localIdentity: async () => ({ ok: true, deviceId: 'pc-a', fingerprint, listenPort: 24800 }),
    };

    render(<App />);
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    fireEvent.click(screen.getByRole('button', { name: 'Show my fingerprint' }));
    expect(await screen.findByText(`pc-a:24800 — ${fingerprint}`)).toBeInTheDocument();
  });

  it('stops and resumes remote input from the dashboard', async () => {
    const stopInput = vi.fn(async () => ({ ok: true, enabled: false }));
    const resumeInput = vi.fn(async () => ({ ok: true, enabled: true }));
    (window as any).mymouse = {
      status: async () => ({ ok: true, connected: true }),
      devices: async () => [],
      pairingUri: async () => 'mymouse://pair?code=123456&id=pc-a',
      approvePairing: async () => ({ ok: true }),
      setLayout: async () => ({ ok: true }),
      getLayout: async () => ({ ok: true, layout: { nodes: [], edges: [] } }),
      getSettings: async () => ({
        ok: true,
        settings: {
          clipboard: { enabled: true, mode: 'global' },
          file_transfer: { auto_accept: false, save_dir: '', max_transfer_bytes: 100 * 1024 * 1024 },
          auto_reconnect: true,
          launch_at_start: false,
          quick_drop_enabled: true,
        },
      }),
      listTransfers: async () => ({ ok: true, transfers: [] }),
      stopInput,
      resumeInput,
    };

    render(<App />);
    expect(await screen.findByText('Input enabled')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Emergency stop' }));
    await waitFor(() => expect(stopInput).toHaveBeenCalled());
    expect(await screen.findByText('Input stopped')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Resume input' }));
    await waitFor(() => expect(resumeInput).toHaveBeenCalled());
    expect(await screen.findByText('Input enabled')).toBeInTheDocument();
  });

  it('renders the backend topology on the Device layout screen', async () => {
    const getLayout = vi.fn(async () => ({
      ok: true,
      layout: {
        nodes: [
          { deviceId: 'pc-a', monitorIndex: 0, width: 1920, height: 1080 },
          { deviceId: 'pc-b', monitorIndex: 0, width: 1280, height: 720 },
        ],
        edges: [{ from: 'pc-a', to: 'pc-b', side: 'right' }],
      },
    }));
    (window as any).mymouse = {
      status: async () => ({ ok: true, connected: true }),
      devices: async () => [],
      pairingUri: async () => 'mymouse://pair?code=123456&id=pc-a',
      approvePairing: async () => ({ ok: true }),
      setLayout: async () => ({ ok: true }),
      getLayout,
      getSettings: async () => ({
        ok: true,
        settings: {
          clipboard: { enabled: true, mode: 'global' },
          file_transfer: { auto_accept: false, save_dir: '', max_transfer_bytes: 100 * 1024 * 1024 },
          auto_reconnect: true,
          launch_at_start: false,
          quick_drop_enabled: true,
        },
      }),
      listTransfers: async () => ({ ok: true, transfers: [] }),
    };

    render(<App />);
    expect(await screen.findByText('Connected')).toBeInTheDocument();
    expect(getLayout).toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Device layout' }));
    expect(await screen.findByText('pc-a : 0')).toBeInTheDocument();
    expect(screen.getByText('pc-b : 0')).toBeInTheDocument();
  });
});

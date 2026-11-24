import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import App from './App';

describe('App', () => {
  it('shows connected state and device list', async () => {
    (window as any).mymouse = {
      status: async () => ({ ok: true, connected: true }),
      devices: async () => [{ name: 'PC-B' }],
      pairingUri: async () => 'mymouse://pair?code=123456&id=pc-a',
      approvePairing: async () => ({ ok: true }),
      setLayout: async () => ({ ok: true }),
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
});

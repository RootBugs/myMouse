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
    };

    render(<App />);
    expect(await screen.findByText('Connected')).toBeInTheDocument();
    expect(screen.getByText('PC-B')).toBeInTheDocument();
  });
});

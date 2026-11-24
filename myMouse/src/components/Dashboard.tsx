import React from 'react';
import type { AppSettings, TransferRecord } from '../types';

type Props = {
  connected: boolean;
  devices: Array<{ name: string; id?: string }>;
  transfers: TransferRecord[];
  settings: AppSettings;
};

export default function Dashboard({ connected, devices, transfers, settings }: Props) {
  const recent = transfers.slice(-5).reverse();
  return (
    <section>
      <h1>Dashboard</h1>
      <div className="status-card">
        <span className="status-label">Status</span>
        <strong>{connected ? 'Connected' : 'Disconnected'}</strong>
      </div>

      <h2>Clipboard sync</h2>
      <div className="status-card">
        <span className="status-label">Mode</span>
        <strong>{settings.clipboard.enabled ? settings.clipboard.mode : 'disabled'}</strong>
      </div>

      <h2>Devices</h2>
      <ul className="device-list">
        {devices.map((device) => (
          <li key={device.id ?? device.name}>{device.name}</li>
        ))}
        {devices.length === 0 && <li className="hint">No trusted devices yet.</li>}
      </ul>

      <h2>Recent transfers</h2>
      <ul className="device-list">
        {recent.map((t) => (
          <li key={t.transfer_id}>
            <span>{t.name}</span>
            <span className="hint">{t.direction} • {t.state}</span>
          </li>
        ))}
        {recent.length === 0 && <li className="hint">No transfer history yet.</li>}
      </ul>

      <h2>Escape hotkey</h2>
      <p className="hint">Ctrl+Alt+Shift+M returns input to this PC.</p>
    </section>
  );
}
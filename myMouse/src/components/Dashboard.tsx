import React from 'react';

type Props = {
  connected: boolean;
  devices: Array<{ name: string; id?: string }>;
};

export default function Dashboard({ connected, devices }: Props) {
  return (
    <section>
      <h1>Dashboard</h1>
      <div className="status-card">
        <span className="status-label">Status</span>
        <strong>{connected ? 'Connected' : 'Disconnected'}</strong>
      </div>
      <h2>Devices</h2>
      <ul className="device-list">
        {devices.map((device) => (
          <li key={device.id ?? device.name}>{device.name}</li>
        ))}
      </ul>
      <h2>Escape hotkey</h2>
      <p className="hint">Ctrl+Alt+Shift+M returns input to this PC.</p>
    </section>
  );
}

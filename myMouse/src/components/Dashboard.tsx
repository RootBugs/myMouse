import React, { useEffect, useState } from 'react';
import type { AppSettings, TransferRecord } from '../types';

type Props = {
  connected: boolean;
  devices: Array<{ name: string; id?: string }>;
  transfers: TransferRecord[];
  settings: AppSettings;
};

type InputStateMessage = Record<string, unknown> & { notification?: string; enabled?: boolean };

export default function Dashboard({ connected, devices, transfers, settings }: Props) {
  const [clipboardText, setClipboardText] = useState('');
  const [clipboardMessage, setClipboardMessage] = useState('');
  const [inputEnabled, setInputEnabled] = useState(true);
  const [inputMessage, setInputMessage] = useState('');
  const recent = transfers.slice(-5).reverse();

  useEffect(() => {
    const unsubscribe = window.mymouse.onNotification?.((message: InputStateMessage) => {
      if (message.notification === 'inputState' && typeof message.enabled === 'boolean') {
        setInputEnabled(message.enabled);
      }
    });
    return () => unsubscribe?.();
  }, []);

  const sendClipboard = async () => {
    const result = await window.mymouse.sendClipboard(clipboardText);
    setClipboardMessage(result.ok ? 'Sent to peer' : (result.error ?? 'Could not send'));
  };

  const stopInput = async () => {
    const result = await window.mymouse.stopInput();
    setInputEnabled(result.enabled);
    setInputMessage(result.ok ? 'Remote input stopped.' : 'Could not stop remote input.');
  };

  const resumeInput = async () => {
    const result = await window.mymouse.resumeInput();
    setInputEnabled(result.enabled);
    setInputMessage(result.ok ? 'Remote input resumed.' : 'Could not resume remote input.');
  };
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
      <div className="status-card">
        <span className="status-label">Send clipboard to peer</span>
        <input
          aria-label="Clipboard text to send"
          value={clipboardText}
          onChange={(e) => setClipboardText(e.target.value)}
        />
        <button disabled={!connected || !settings.clipboard.enabled} onClick={() => void sendClipboard()}>
          Send
        </button>
        {!connected && <span className="hint">Connect to a pinned peer first.</span>}
        {clipboardMessage && <span className="hint">{clipboardMessage}</span>}
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

      <h2>Remote input</h2>
      <div className="status-card">
        <span className="status-label">Injection gate</span>
        <strong>{inputEnabled ? 'Input enabled' : 'Input stopped'}</strong>
      </div>
      <div className="status-card">
        {inputEnabled ? (
          <button onClick={() => void stopInput()}>Emergency stop</button>
        ) : (
          <button onClick={() => void resumeInput()}>Resume input</button>
        )}
        {inputMessage && <span className="hint">{inputMessage}</span>}
      </div>
      <p className="hint">Emergency stop closes the injection gate immediately. Remote hotkeys are rejected.</p>
    </section>
  );
}
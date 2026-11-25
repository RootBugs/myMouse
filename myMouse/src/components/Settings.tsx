import React, { useState } from 'react';
import type { AppSettings } from '../types';

type Props = {
  settings: AppSettings;
  onChange: (settings: AppSettings) => void;
  onRefresh: () => Promise<void>;
};

export default function Settings({ settings, onChange, onRefresh }: Props) {
  const [saveDir, setSaveDir] = useState(settings.file_transfer.save_dir);
  const [peerAddress, setPeerAddress] = useState('');
  const [peerId, setPeerId] = useState('');
  const [fingerprint, setFingerprint] = useState('');
  const [connectionMessage, setConnectionMessage] = useState('');
  const [localFingerprint, setLocalFingerprint] = useState('');

  const showLocalIdentity = async () => {
    const identity = await window.mymouse.localIdentity();
    if (identity.ok && identity.fingerprint) {
      setLocalFingerprint(`${identity.deviceId ?? 'local-pc'}:${identity.listenPort ?? 0} — ${identity.fingerprint}`);
    } else {
      setLocalFingerprint(identity.error ?? 'Could not read local identity.');
    }
  };

  const connectPeer = async () => {
    if (!/^[0-9a-fA-F]{64}$/.test(fingerprint.trim())) {
      setConnectionMessage('Fingerprint must be 64 hex characters.');
      return;
    }
    const result = await window.mymouse.connectPeer(peerAddress.trim(), peerId.trim(), fingerprint.trim());
    setConnectionMessage(result.ok ? 'Connected to pinned peer.' : (result.error ?? 'Connection failed.'));
    await onRefresh();
  };

  const updateClipboard = async (patch: { enabled?: boolean; mode?: 'global' | 'cursor_scoped' }) => {
    const result = await window.mymouse.setClipboardSettings(patch);
    if (result?.settings) {
      onChange({ ...settings, clipboard: result.settings });
    }
    await onRefresh();
  };

  const updateFile = async (patch: { autoAccept?: boolean; saveDir?: string; maxTransferBytes?: number }) => {
    const result = await window.mymouse.setFileTransferSettings(patch);
    if (result?.settings) {
      onChange({ ...settings, file_transfer: result.settings });
    }
    await onRefresh();
  };

  return (
    <section>
      <h1>Settings</h1>
      <label>Language <select defaultValue="en"><option value="en">English</option></select></label>

      <h2>Clipboard sync</h2>
      <label>
        <input
          type="checkbox"
          checked={settings.clipboard.enabled}
          onChange={(e) => void updateClipboard({ enabled: e.target.checked })}
        />{' '}
        Enable clipboard sync
      </label>
      <label>
        Mode{' '}
        <select
          value={settings.clipboard.mode}
          onChange={(e) => void updateClipboard({ mode: e.target.value as 'global' | 'cursor_scoped' })}
        >
          <option value="global">Global last-write-wins</option>
          <option value="cursor_scoped">Cursor-scoped</option>
        </select>
      </label>

      <h2>File transfer</h2>
      <label>
        <input
          type="checkbox"
          checked={settings.file_transfer.auto_accept}
          onChange={(e) => void updateFile({ autoAccept: e.target.checked })}
        />{' '}
        Auto-accept files from trusted peers
      </label>
      <label>
        Save folder
        <input
          aria-label="Save folder"
          value={saveDir}
          onChange={(e) => setSaveDir(e.target.value)}
          onBlur={() => void updateFile({ saveDir })}
        />
      </label>
      <label>
        Max transfer size (MB)
        <input
          aria-label="Max transfer size (MB)"
          type="number"
          min={1}
          value={Math.round(settings.file_transfer.max_transfer_bytes / (1024 * 1024))}
          onChange={(e) => {
            const mb = Math.max(1, Number(e.target.value || 1));
            void updateFile({ maxTransferBytes: mb * 1024 * 1024 });
          }}
        />
      </label>

      <h2>Peer connection</h2>
      <p className="hint">Connect only to a peer whose certificate fingerprint you have verified.</p>
      <button onClick={() => void showLocalIdentity()}>Show my fingerprint</button>
      {localFingerprint && <p className="hint">{localFingerprint}</p>}
      <label>
        Address (host:port)
        <input aria-label="Peer address" value={peerAddress} onChange={(e) => setPeerAddress(e.target.value)} />
      </label>
      <label>
        Peer device ID
        <input aria-label="Peer device ID" value={peerId} onChange={(e) => setPeerId(e.target.value)} />
      </label>
      <label>
        Pinned SHA-256 fingerprint
        <input aria-label="Pinned fingerprint" value={fingerprint} onChange={(e) => setFingerprint(e.target.value)} />
      </label>
      <button onClick={() => void connectPeer()}>Connect with pinned fingerprint</button>
      {connectionMessage && <p className="hint">{connectionMessage}</p>}

      <h2>General</h2>
      <label><input type="checkbox" checked={settings.auto_reconnect} readOnly /> Auto reconnect</label>
      <label><input type="checkbox" checked={false} readOnly /> Launch at Windows start (off until you enable it)</label>
      <label><input type="checkbox" checked={settings.quick_drop_enabled} readOnly /> Quick Drop zone</label>
      <p className="hint">Escape hotkey: Ctrl+Alt+Shift+M</p>
      <p className="hint">v1 cannot inject into UAC prompts, secure desktop, or locked sessions.</p>
    </section>
  );
}
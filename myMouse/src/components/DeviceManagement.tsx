import React, { useCallback, useEffect, useState } from 'react';
import { QRCodeSVG } from 'qrcode.react';
import type { DiscoveredPeer } from '../types';

type Props = { devices: Array<{ name: string; id?: string }> };

function shortFingerprint(fingerprint: string): string {
  return fingerprint.length > 12 ? `${fingerprint.slice(0, 12)}…` : fingerprint;
}

export default function DeviceManagement({ devices }: Props) {
  const [uri, setUri] = useState('');
  const [code, setCode] = useState('');
  const [peers, setPeers] = useState<DiscoveredPeer[]>([]);
  const [pairMessage, setPairMessage] = useState('');

  const refreshPeers = useCallback(async () => {
    try {
      const result = await window.mymouse.discoveredPeers();
      setPeers(result?.peers ?? []);
    } catch {
      // Backend may not be available in tests/dev; keep the list empty.
      setPeers([]);
    }
  }, []);

  useEffect(() => {
    window.mymouse.pairingUri().then(setUri).catch(() => setUri(''));
    void refreshPeers();
    const unsubscribe = window.mymouse.onNotification?.((message) => {
      if (message.notification === 'discoveredPeer') void refreshPeers();
    });
    return () => unsubscribe?.();
  }, [refreshPeers]);

  const pair = async (peer: DiscoveredPeer) => {
    try {
      const result = await window.mymouse.pairDiscovered(peer.id);
      setPairMessage(result.ok ? `Paired with ${peer.name}.` : (result.error ?? 'Pairing failed.'));
    } catch {
      setPairMessage('Pairing failed.');
    }
    await refreshPeers();
  };

  return (
    <section>
      <h1>Device management</h1>
      <div className="pairing-card">
        <h2>Pair a device</h2>
        {uri && <QRCodeSVG value={uri} size={160} />}
        <p className="hint">Scan the QR or enter the 6-digit code shown on the joining device.</p>
        <input
          aria-label="Pairing code"
          value={code}
          onChange={(event) => setCode(event.target.value.replace(/\D/g, '').slice(0, 6))}
          placeholder="000000"
        />
        <button onClick={() => window.mymouse.approvePairing(code)} disabled={code.length !== 6}>
          Approve
        </button>
      </div>

      <h2>Discovered peers</h2>
      {peers.length === 0 ? (
        <p className="hint">No discovered peers yet.</p>
      ) : (
        <ul className="device-list">
          {peers.map((peer) => (
            <li key={peer.id} className="transfer-row">
              <div className="transfer-main">
                <strong>{peer.name}</strong>
                <span className="hint">{peer.id}</span>
                <span className="hint">{peer.address}:{peer.port}</span>
                <span className="hint">Fingerprint: {shortFingerprint(peer.fingerprint)}</span>
              </div>
              <div className="transfer-actions">
                <button onClick={() => void pair(peer)}>Pair</button>
              </div>
            </li>
          ))}
        </ul>
      )}
      {pairMessage && <p className="hint">{pairMessage}</p>}

      <h2>Trusted devices</h2>
      <ul className="device-list">
        {devices.map((device) => (
          <li key={device.id ?? device.name}>
            <span>{device.name}</span>
            <label><input type="checkbox" defaultChecked /> Input</label>
            <label><input type="checkbox" defaultChecked /> Clipboard</label>
            <label><input type="checkbox" defaultChecked /> Files</label>
            <label><input type="checkbox" defaultChecked /> Screen</label>
            <button onClick={() => undefined}>Revoke</button>
          </li>
        ))}
      </ul>
    </section>
  );
}

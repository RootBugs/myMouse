import React, { useEffect, useState } from 'react';
import { QRCodeSVG } from 'qrcode.react';

type Props = { devices: Array<{ name: string; id?: string }> };

export default function DeviceManagement({ devices }: Props) {
  const [uri, setUri] = useState('');
  const [code, setCode] = useState('');

  useEffect(() => {
    window.mymouse.pairingUri().then(setUri).catch(() => setUri(''));
  }, []);

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

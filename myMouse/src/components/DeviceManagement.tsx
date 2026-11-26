import React, { useCallback, useEffect, useState } from 'react';
import { QRCodeSVG } from 'qrcode.react';
import type { DeviceCapabilities, DiscoveredPeer, TrustedDevice } from '../types';

type Props = {
  devices: TrustedDevice[];
  onRefresh: () => Promise<void>;
};

type FingerprintConfirmation = { fingerprint: string; verified: boolean };

const allCapabilities: DeviceCapabilities = {
  input: true,
  clipboard: true,
  files: true,
  screen: true,
};

function shortFingerprint(fingerprint: string): string {
  return fingerprint.length > 12 ? `${fingerprint.slice(0, 12)}…` : fingerprint;
}

function isFingerprint(value: string): boolean {
  return /^[0-9a-fA-F]{64}$/.test(value.trim());
}

function isPeerId(value: string): boolean {
  return /^[A-Za-z0-9_-]{1,128}$/.test(value.trim());
}

export default function DeviceManagement({ devices, onRefresh }: Props) {
  const [uri, setUri] = useState('');
  const [code, setCode] = useState('');
  const [approvePeerId, setApprovePeerId] = useState('');
  const [approveFingerprint, setApproveFingerprint] = useState('');
  const [approveVerified, setApproveVerified] = useState(false);
  const [pairMessage, setPairMessage] = useState('');
  const [approveMessage, setApproveMessage] = useState('');
  const [deviceMessage, setDeviceMessage] = useState('');
  const [peers, setPeers] = useState<DiscoveredPeer[]>([]);
  const [confirmations, setConfirmations] = useState<Record<string, FingerprintConfirmation>>({});

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

  const confirmationFor = (peerId: string): FingerprintConfirmation =>
    confirmations[peerId] ?? { fingerprint: '', verified: false };

  const updateConfirmation = (peerId: string, patch: Partial<FingerprintConfirmation>) => {
    setConfirmations((previous) => ({
      ...previous,
      [peerId]: { ...confirmationFor(peerId), ...patch },
    }));
  };

  const pair = async (peer: DiscoveredPeer) => {
    const confirmation = confirmationFor(peer.id);
    if (!isFingerprint(confirmation.fingerprint) || !confirmation.verified) {
      setPairMessage('Enter and verify the full fingerprint before pairing.');
      return;
    }
    try {
      const result = await window.mymouse.pairDiscovered(
        peer.id,
        confirmation.fingerprint.trim(),
        true,
      );
      setPairMessage(result.ok ? `Paired with ${peer.name}.` : (result.error ?? 'Pairing failed.'));
    } catch {
      setPairMessage('Pairing failed.');
    }
    await onRefresh();
    await refreshPeers();
  };

  const approve = async () => {
    if (!isPeerId(approvePeerId) || !isFingerprint(approveFingerprint) || !approveVerified) {
      setApproveMessage('Confirm the joining peer ID and full fingerprint before approving.');
      return;
    }
    try {
      const result = await window.mymouse.approvePairing(
        code.trim(),
        approvePeerId.trim(),
        approveFingerprint.trim(),
        true,
      );
      setApproveMessage(result.ok ? 'Pairing approved.' : (result.error ?? 'Approval failed.'));
    } catch {
      setApproveMessage('Approval failed.');
    }
    await onRefresh();
  };

  const setCapability = async (
    device: TrustedDevice,
    key: keyof DeviceCapabilities,
    value: boolean,
  ) => {
    const id = device.id ?? device.name;
    const next = { ...(device.capabilities ?? allCapabilities), [key]: value };
    try {
      const result = await window.mymouse.setDeviceCapabilities(
        id,
        next.input,
        next.clipboard,
        next.files,
        next.screen,
      );
      setDeviceMessage(result.ok ? `Updated ${device.name}.` : (result.error ?? 'Update failed.'));
    } catch {
      setDeviceMessage('Update failed.');
    }
    await onRefresh();
  };

  const revoke = async (device: TrustedDevice) => {
    const id = device.id ?? device.name;
    try {
      const result = await window.mymouse.revokeDevice(id);
      setDeviceMessage(result.ok ? `Revoked ${device.name}.` : (result.error ?? 'Revoke failed.'));
    } catch {
      setDeviceMessage('Revoke failed.');
    }
    await onRefresh();
  };

  const canApprove = code.length === 6
    && isPeerId(approvePeerId)
    && isFingerprint(approveFingerprint)
    && approveVerified;

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
        <label>
          Joining peer device ID
          <input
            aria-label="Pairing peer ID"
            value={approvePeerId}
            onChange={(event) => setApprovePeerId(event.target.value)}
          />
        </label>
        <label>
          Joining peer full fingerprint
          <input
            aria-label="Pairing peer fingerprint"
            value={approveFingerprint}
            onChange={(event) => setApproveFingerprint(event.target.value.trim())}
          />
        </label>
        <label>
          <input
            type="checkbox"
            aria-label="Verify joining peer fingerprint"
            checked={approveVerified}
            onChange={(event) => setApproveVerified(event.target.checked)}
          />{' '}
          I verified this fingerprint out of band
        </label>
        <button onClick={() => void approve()} disabled={!canApprove}>Approve</button>
        {approveMessage && <p className="hint">{approveMessage}</p>}
      </div>

      <h2>Discovered peers</h2>
      {peers.length === 0 ? (
        <p className="hint">No discovered peers yet.</p>
      ) : (
        <ul className="device-list">
          {peers.map((peer) => {
            const confirmation = confirmationFor(peer.id);
            const canPair = isFingerprint(confirmation.fingerprint)
              && confirmation.fingerprint.trim().toLowerCase() === peer.fingerprint.toLowerCase()
              && confirmation.verified;
            return (
              <li key={peer.id} className="transfer-row">
                <div className="transfer-main">
                  <strong>{peer.name}</strong>
                  <span className="hint">{peer.id}</span>
                  <span className="hint">{peer.address}:{peer.port}</span>
                  <span className="hint">Fingerprint: {shortFingerprint(peer.fingerprint)}</span>
                  <code className="fingerprint">{peer.fingerprint}</code>
                  <label>
                    Confirm full fingerprint
                    <input
                      aria-label={`Confirm fingerprint for ${peer.name}`}
                      value={confirmation.fingerprint}
                      onChange={(event) => updateConfirmation(peer.id, {
                        fingerprint: event.target.value.trim(),
                      })}
                    />
                  </label>
                  <label>
                    <input
                      type="checkbox"
                      aria-label={`Verified ${peer.name} fingerprint`}
                      checked={confirmation.verified}
                      onChange={(event) => updateConfirmation(peer.id, {
                        verified: event.target.checked,
                      })}
                    />{' '}
                    I verified this fingerprint out of band
                  </label>
                </div>
                <div className="transfer-actions">
                  <button disabled={!canPair} onClick={() => void pair(peer)}>Pair</button>
                </div>
              </li>
            );
          })}
        </ul>
      )}
      {pairMessage && <p className="hint">{pairMessage}</p>}

      <h2>Trusted devices</h2>
      <ul className="device-list">
        {devices.map((device) => {
          const capabilities = device.capabilities ?? allCapabilities;
          return (
            <li key={device.id ?? device.name}>
              <span>{device.name}</span>
              {(['input', 'clipboard', 'files', 'screen'] as const).map((key) => (
                <label key={key}>
                  <input
                    type="checkbox"
                    aria-label={`${key[0].toUpperCase()}${key.slice(1)} for ${device.name}`}
                    checked={capabilities[key]}
                    onChange={(event) => void setCapability(device, key, event.target.checked)}
                  />{' '}
                  {key[0].toUpperCase()}{key.slice(1)}
                </label>
              ))}
              <button aria-label={`Revoke ${device.name}`} onClick={() => void revoke(device)}>
                Revoke
              </button>
            </li>
          );
        })}
      </ul>
      {deviceMessage && <p className="hint">{deviceMessage}</p>}
    </section>
  );
}

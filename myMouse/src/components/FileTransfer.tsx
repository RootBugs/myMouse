import React, { useCallback, useState } from 'react';
import type { AppSettings, TransferRecord } from '../types';

type Props = {
  transfers: TransferRecord[];
  settings: AppSettings;
  onRefresh: () => Promise<void>;
};

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export default function FileTransfer({ transfers, settings, onRefresh }: Props) {
  const [dragging, setDragging] = useState(false);
  const [message, setMessage] = useState('');

  const offerPaths = useCallback(async (paths: string[]) => {
    for (const path of paths) {
      const result = await window.mymouse.offerFile(path);
      if (!result.ok) {
        setMessage(result.error ?? 'Failed to offer file');
      } else {
        setMessage(`Offered ${path}`);
      }
    }
    await onRefresh();
  }, [onRefresh]);

  const onDrop = useCallback(async (event: React.DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    setDragging(false);
    const files = Array.from(event.dataTransfer.files);
    const paths = files
      .map((file) => (file as File & { path?: string }).path)
      .filter((p): p is string => Boolean(p));
    if (paths.length === 0) {
      setMessage('Drop a file from the file system to offer it.');
      return;
    }
    await offerPaths(paths);
  }, [offerPaths]);

  return (
    <section>
      <h1>File transfer</h1>
      <p className="hint">
        Files from trusted peers auto-save to {settings.file_transfer.save_dir || 'the configured folder'} when auto-accept is enabled.
      </p>

      <div
        className={dragging ? 'drop-zone dragging' : 'drop-zone'}
        onDragOver={(e) => { e.preventDefault(); setDragging(true); }}
        onDragLeave={() => setDragging(false)}
        onDrop={onDrop}
        role="region"
        aria-label="Quick Drop zone"
      >
        <strong>Quick Drop</strong>
        <p className="hint">Drop files here to send them to a trusted peer.</p>
        <p className="hint">Max size: {formatBytes(settings.file_transfer.max_transfer_bytes)}</p>
      </div>

      {message && <p className="hint">{message}</p>}

      <h2>Transfer history</h2>
      {transfers.length === 0 ? (
        <p className="hint">No transfers yet.</p>
      ) : (
        <ul className="device-list">
          {transfers.slice().reverse().map((t) => (
            <li key={t.transfer_id} className="transfer-row">
              <div className="transfer-main">
                <strong>{t.name}</strong>
                <span className="hint">{t.direction} • {t.state} • {formatBytes(t.bytes_transferred)} / {formatBytes(t.size)}</span>
                {t.message && <span className="hint">{t.message}</span>}
              </div>
              <div className="transfer-actions">
                {t.direction === 'incoming' && t.state === 'offered' && (
                  <>
                    <button onClick={async () => { await window.mymouse.acceptTransfer(t.transfer_id); await onRefresh(); }}>Accept</button>
                    <button onClick={async () => { await window.mymouse.rejectTransfer(t.transfer_id, 'rejected by user'); await onRefresh(); }}>Reject</button>
                  </>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
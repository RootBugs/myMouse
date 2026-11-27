export interface ClipboardSettings {
  enabled: boolean;
  mode: 'global' | 'cursor_scoped';
}

export interface FileTransferSettings {
  auto_accept: boolean;
  save_dir: string;
  max_transfer_bytes: number;
}

export interface AppSettings {
  clipboard: ClipboardSettings;
  file_transfer: FileTransferSettings;
  auto_reconnect: boolean;
  launch_at_start: boolean;
  quick_drop_enabled: boolean;
}

export interface TransferRecord {
  transfer_id: string;
  direction: 'incoming' | 'outgoing';
  peer_device_id: string;
  name: string;
  size: number;
  sha256: string;
  state: 'offered' | 'accepted' | 'rejected' | 'in_progress' | 'completed' | 'cancelled' | 'failed';
  bytes_transferred: number;
  path?: string | null;
  message?: string | null;
  updated_at_ms: number;
}

export interface DiscoveredPeer {
  id: string;
  name: string;
  address: string;
  port: number;
  fingerprint: string;
}

export interface DeviceCapabilities {
  input: boolean;
  clipboard: boolean;
  files: boolean;
  screen: boolean;
}

export interface TrustedDevice {
  name: string;
  id?: string;
  capabilities?: DeviceCapabilities;
}

export type LayoutSide = 'left' | 'right' | 'top' | 'bottom';

export interface LayoutNode {
  deviceId: string;
  monitorIndex: number;
  width: number;
  height: number;
}

export interface LayoutEdge {
  from: string;
  to: string;
  side: LayoutSide;
}

export interface DeviceLayoutData {
  nodes: LayoutNode[];
  edges: LayoutEdge[];
}

declare global {
  interface Window {
    mymouse: {
      status: () => Promise<{ ok: boolean; connected: boolean; deviceId?: string }>;
      devices: () => Promise<TrustedDevice[]>;
      pairingUri: (peerId: string, fingerprint: string) => Promise<string>;
      approvePairing: (code: string, peerId: string, fingerprint: string, verified: boolean) =>
        Promise<{ ok: boolean; error?: string }>;
      setLayout: (layout: DeviceLayoutData) => Promise<{ ok: boolean; error?: string }>;
      getLayout: () => Promise<{ ok: boolean; layout: DeviceLayoutData; error?: string }>;

      getSettings: () => Promise<{ ok: boolean; settings: AppSettings }>;
      setClipboardSettings: (settings: { enabled?: boolean; mode?: 'global' | 'cursor_scoped' }) =>
        Promise<{ ok: boolean; settings?: ClipboardSettings }>;
      setFileTransferSettings: (settings: { autoAccept?: boolean; saveDir?: string; maxTransferBytes?: number }) =>
        Promise<{ ok: boolean; settings?: FileTransferSettings }>;

      listTransfers: () => Promise<{ ok: boolean; transfers: TransferRecord[] }>;
      acceptTransfer: (transferId: string) => Promise<{ ok: boolean; error?: string }>;
      rejectTransfer: (transferId: string, reason?: string) => Promise<{ ok: boolean; error?: string }>;
      offerFile: (path: string, peerDeviceId?: string) => Promise<{ ok: boolean; offer?: unknown; error?: string }>;
      sendClipboard: (text: string) => Promise<{ ok: boolean; error?: string }>;
      connectPeer: (address: string, peerDeviceId: string, fingerprint: string) => Promise<{ ok: boolean; error?: string }>;
      discoveredPeers: () => Promise<{ ok: boolean; peers: DiscoveredPeer[] }>;
      pairDiscovered: (peerId: string, fingerprint: string, verified: boolean) =>
        Promise<{ ok: boolean; error?: string }>;
      localIdentity: () => Promise<{ ok: boolean; deviceId?: string; fingerprint?: string; listenPort?: number; error?: string }>;

      stopInput: () => Promise<{ ok: boolean; enabled: boolean }>;
      emergencyStop: () => Promise<{ ok: boolean; enabled: boolean }>;
      resumeInput: () => Promise<{ ok: boolean; enabled: boolean }>;
      revokeDevice: (peerId: string) => Promise<{ ok: boolean; error?: string }>;
      revokePairing: (peerId: string) => Promise<{ ok: boolean; error?: string }>;
      setDeviceCapabilities: (
        peerId: string,
        input: boolean,
        clipboard: boolean,
        files: boolean,
        screen: boolean,
      ) => Promise<{ ok: boolean; error?: string }>;

      onNotification?: (cb: (message: Record<string, unknown>) => void) => () => void;
    };
  }
}

export {};
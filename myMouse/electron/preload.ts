import { contextBridge, ipcRenderer } from 'electron';

contextBridge.exposeInMainWorld('mymouse', {
  status: () => ipcRenderer.invoke('mymouse:call', 'status'),
  devices: () => ipcRenderer.invoke('mymouse:call', 'devices'),
  pairingUri: () => ipcRenderer.invoke('mymouse:call', 'pairingUri'),
  approvePairing: (code: string, peerId: string, fingerprint: string, verified: boolean) =>
    ipcRenderer.invoke('mymouse:call', 'approvePairing', { code, peerId, fingerprint, verified }),
  setLayout: (layout: unknown) => ipcRenderer.invoke('mymouse:call', 'setLayout', { layout }),

  getSettings: () => ipcRenderer.invoke('mymouse:call', 'getSettings'),
  setClipboardSettings: (settings: { enabled?: boolean; mode?: 'global' | 'cursor_scoped' }) =>
    ipcRenderer.invoke('mymouse:call', 'setClipboardSettings', settings),
  setFileTransferSettings: (settings: { autoAccept?: boolean; saveDir?: string; maxTransferBytes?: number }) =>
    ipcRenderer.invoke('mymouse:call', 'setFileTransferSettings', settings),

  listTransfers: () => ipcRenderer.invoke('mymouse:call', 'listTransfers'),
  acceptTransfer: (transferId: string) => ipcRenderer.invoke('mymouse:call', 'acceptTransfer', { transferId }),
  rejectTransfer: (transferId: string, reason?: string) =>
    ipcRenderer.invoke('mymouse:call', 'rejectTransfer', { transferId, reason }),
  offerFile: (path: string, peerDeviceId?: string) =>
    ipcRenderer.invoke('mymouse:call', 'offerFile', { path, peerDeviceId }),
  sendClipboard: (text: string) =>
    ipcRenderer.invoke('mymouse:call', 'sendClipboard', { text }),
  connectPeer: (address: string, peerDeviceId: string, fingerprint: string) =>
    ipcRenderer.invoke('mymouse:call', 'connectPeer', { address, peerDeviceId, fingerprint }),
  discoveredPeers: () => ipcRenderer.invoke('mymouse:call', 'discoveredPeers'),
  pairDiscovered: (peerId: string, fingerprint: string, verified: boolean) =>
    ipcRenderer.invoke('mymouse:call', 'pairDiscovered', { peerId, fingerprint, verified }),
  localIdentity: () => ipcRenderer.invoke('mymouse:call', 'localIdentity'),

  stopInput: () => ipcRenderer.invoke('mymouse:call', 'stopInput'),
  emergencyStop: () => ipcRenderer.invoke('mymouse:call', 'emergencyStop'),
  resumeInput: () => ipcRenderer.invoke('mymouse:call', 'resumeInput'),
  revokeDevice: (peerId: string) => ipcRenderer.invoke('mymouse:call', 'revokeDevice', { peerId }),
  revokePairing: (peerId: string) => ipcRenderer.invoke('mymouse:call', 'revokePairing', { peerId }),
  setDeviceCapabilities: (
    peerId: string,
    input: boolean,
    clipboard: boolean,
    files: boolean,
    screen: boolean,
  ) => ipcRenderer.invoke('mymouse:call', 'setDeviceCapabilities', {
    peerId,
    input,
    clipboard,
    files,
    screen,
  }),

  onNotification: (cb: (message: unknown) => void) => {
    const listener = (_event: unknown, message: unknown) => cb(message);
    ipcRenderer.on('mymouse:notification', listener);
    return () => ipcRenderer.removeListener('mymouse:notification', listener);
  },
});

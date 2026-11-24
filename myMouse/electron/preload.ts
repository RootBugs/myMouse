import { contextBridge, ipcRenderer } from 'electron';

contextBridge.exposeInMainWorld('mymouse', {
  status: () => ipcRenderer.invoke('mymouse:call', 'status'),
  devices: () => ipcRenderer.invoke('mymouse:call', 'devices'),
  pairingUri: () => ipcRenderer.invoke('mymouse:call', 'pairingUri'),
  approvePairing: (code: string) => ipcRenderer.invoke('mymouse:call', 'approvePairing', { code }),
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
});

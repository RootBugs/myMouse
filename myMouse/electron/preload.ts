import { contextBridge, ipcRenderer } from 'electron';

contextBridge.exposeInMainWorld('mymouse', {
  status: () => ipcRenderer.invoke('mymouse:call', 'status'),
  devices: () => ipcRenderer.invoke('mymouse:call', 'devices'),
  pairingUri: () => ipcRenderer.invoke('mymouse:call', 'pairingUri'),
  approvePairing: (code: string) => ipcRenderer.invoke('mymouse:call', 'approvePairing', { code }),
  setLayout: (layout: unknown) => ipcRenderer.invoke('mymouse:call', 'setLayout', { layout }),
});

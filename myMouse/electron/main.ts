import { app, BrowserWindow, ipcMain } from 'electron';
import { spawn, type ChildProcess } from 'node:child_process';
import path from 'node:path';
import { Bridge } from './bridge';

let backend: ChildProcess | null = null;
let bridge: Bridge | null = null;

function startBackend(): void {
  const exe = app.isPackaged
    ? path.join(process.resourcesPath, 'mymouse-backend.exe')
    : path.join(__dirname, '..', 'backend', 'target', 'debug', 'mymouse-backend.exe');
  backend = spawn(exe, ['--ipc', 'stdio'], { stdio: ['pipe', 'pipe', 'pipe'] });
  bridge = new Bridge(backend as unknown as ConstructorParameters<typeof Bridge>[0]);
  bridge.onNotification((message) => {
    for (const window of BrowserWindow.getAllWindows()) {
      window.webContents.send('mymouse:notification', message);
    }
  });

  ipcMain.handle('mymouse:call', (_event, method: string, params: unknown) => {
    return bridge!.send(method, params);
  });
}

function createWindow(): void {
  const window = new BrowserWindow({
    width: 1100,
    height: 720,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  window.loadFile(path.join(__dirname, '..', 'dist', 'index.html'));
}

app.whenReady().then(() => {
  startBackend();
  createWindow();
});

app.on('window-all-closed', () => {
  backend?.kill();
  if (process.platform !== 'darwin') app.quit();
});

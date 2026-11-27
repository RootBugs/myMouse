import { app, BrowserWindow, ipcMain } from 'electron';
import { spawn, type ChildProcess } from 'node:child_process';
import path from 'node:path';
import { Bridge } from './bridge';

const BACKEND_IPC_TIMEOUT_MS = 30_000;

let backend: ChildProcess | null = null;
let bridge: Bridge | null = null;
let backendAvailable = true;

function notify(message: unknown): void {
  for (const window of BrowserWindow.getAllWindows()) {
    window.webContents.send('mymouse:notification', message);
  }
}

function handleBackendGone(reason: string, notification: unknown): void {
  if (!backendAvailable) return;
  backendAvailable = false;
  bridge?.close(reason);
  notify(notification);
}

function startBackend(): void {
  const exe = app.isPackaged
    ? path.join(process.resourcesPath, 'mymouse-backend.exe')
    : path.join(__dirname, '..', 'backend', 'target', 'debug', 'mymouse-backend.exe');
  backend = spawn(exe, ['--ipc', 'stdio'], { stdio: ['pipe', 'pipe', 'pipe'] });
  bridge = new Bridge(backend as unknown as ConstructorParameters<typeof Bridge>[0], {
    timeoutMs: BACKEND_IPC_TIMEOUT_MS,
  });
  bridge.onNotification(notify);

  backend.on('error', (error: Error) => {
    handleBackendGone(`backend error: ${error.message}`, {
      notification: 'backendError',
      error: error.message,
    });
  });
  backend.on('exit', (code, signal) => {
    handleBackendGone('backend exited', {
      notification: 'backendExit',
      code,
      signal,
    });
  });
  backend.on('close', (code, signal) => {
    handleBackendGone('backend exited', {
      notification: 'backendExit',
      code,
      signal,
    });
  });

  ipcMain.handle('mymouse:call', (_event, method: string, params: unknown) => {
    if (!backendAvailable || !bridge) {
      return Promise.reject(new Error('backend exited'));
    }
    return bridge.send(method, params);
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

app.on('activate', () => {
  if (BrowserWindow.getAllWindows().length === 0) createWindow();
});

app.on('window-all-closed', () => {
  backend?.kill();
  if (process.platform !== 'darwin') app.quit();
});

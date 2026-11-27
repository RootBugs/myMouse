import { describe, it, expect, vi } from 'vitest';

const mocks = vi.hoisted(() => {
  const ipcHandlers = new Map<string, (...args: any[]) => unknown>();
  const childHandlers = new Map<string, (...args: any[]) => void>();
  const notifications: unknown[] = [];
  const child = {
    stdin: { write: vi.fn(() => true) },
    stdout: { on: vi.fn() },
    on: vi.fn((event: string, cb: (...args: any[]) => void) => {
      childHandlers.set(event, cb);
    }),
    kill: vi.fn(),
  };
  const spawn = vi.fn(() => child);
  return { ipcHandlers, childHandlers, notifications, child, spawn };
});

vi.mock('electron', () => {
  class BrowserWindow {
    webContents = { send: (channel: string, message: unknown) => {
      if (channel === 'mymouse:notification') mocks.notifications.push(message);
    } };
    loadFile() {}
    static getAllWindows() {
      return [new BrowserWindow()];
    }
  }
  return {
    app: {
      isPackaged: false,
      whenReady: vi.fn(async () => {}),
      on: vi.fn(),
      quit: vi.fn(),
    },
    BrowserWindow,
    ipcMain: {
      handle: vi.fn((name: string, cb: (...args: any[]) => unknown) => {
        mocks.ipcHandlers.set(name, cb);
      }),
    },
  };
});

vi.mock('node:child_process', () => ({
  spawn: mocks.spawn,
  default: { spawn: mocks.spawn },
}));

describe('Electron main backend lifecycle', () => {
  it('reports a missing backend executable without an unhandled ENOENT', async () => {
    await import('./main');
    await Promise.resolve();

    expect(mocks.childHandlers.has('error')).toBe(true);
    const handler = mocks.childHandlers.get('error');
    const error = Object.assign(new Error('spawn mymouse-backend.exe ENOENT'), { code: 'ENOENT' });

    expect(() => handler?.(error)).not.toThrow();
    expect(mocks.notifications).toContainEqual({
      notification: 'backendError',
      error: expect.stringContaining('ENOENT'),
    });
  });
});

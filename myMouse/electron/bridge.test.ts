import { describe, it, expect, vi } from 'vitest';
import { EventEmitter } from 'node:events';
import { Bridge, type IpcProcess } from './bridge';

function makeMockBackend(): IpcProcess & { emitLine: (line: string) => void; exit: () => void } {
  const emitter = new EventEmitter();
  const exitCallbacks: Array<() => void> = [];
  const proc = {
    stdin: {
      write: (line: string) => {
        const request = JSON.parse(line.trim());
        process.nextTick(() => {
          emitter.emit('data', JSON.stringify({ id: request.id, result: { ok: true } }) + '\n');
        });
        return true;
      },
    },
    stdout: emitter as unknown as IpcProcess['stdout'],
    onExit: (cb: () => void) => {
      exitCallbacks.push(cb);
    },
    emitLine: (line: string) => {
      emitter.emit('data', line + '\n');
    },
    exit: () => {
      for (const cb of exitCallbacks) cb();
    },
  };
  return proc;
}

describe('Bridge', () => {
  it('round trips a status request', async () => {
    const bridge = new Bridge(makeMockBackend());
    await expect(bridge.send('status')).resolves.toMatchObject({ ok: true });
  });

  it('rejects pending sends when the backend exits', async () => {
    const backend = makeMockBackend();
    const bridge = new Bridge(backend);
    const pending = bridge.send('status');

    backend.exit();

    await expect(pending).rejects.toThrow(/backend exited/i);
    await expect(bridge.send('status')).rejects.toThrow(/backend exited/i);
  });

  it('rejects a pending send after the configured timeout', async () => {
    const backend = makeMockBackend();
    backend.stdin.write = () => true;
    const bridge = new Bridge(backend, { timeoutMs: 20 });

    await expect(bridge.send('slow')).rejects.toThrow(/timed out/i);
  });

  it('ignores malformed replies and does not resolve duplicate ids twice', async () => {
    const backend = makeMockBackend();
    const bridge = new Bridge(backend);
    const notifications: unknown[] = [];
    bridge.onNotification((message) => notifications.push(message));

    await expect(bridge.send('status')).resolves.toMatchObject({ ok: true });
    expect(() => {
      backend.emitLine('null');
      backend.emitLine('42');
      backend.emitLine(JSON.stringify({ id: 'not-a-number', result: { ok: true } }));
      backend.emitLine(JSON.stringify({ id: 1, result: { ok: false, error: 'duplicate' } }));
    }).not.toThrow();

    expect(notifications).toHaveLength(4);
    for (const message of notifications) {
      expect(message).toMatchObject({ notification: 'malformedIpcMessage' });
    }
  });
});

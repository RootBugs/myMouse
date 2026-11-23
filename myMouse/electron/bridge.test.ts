import { describe, it, expect } from 'vitest';
import { EventEmitter } from 'node:events';
import { Bridge, type IpcProcess } from './bridge';

function makeMockBackend(): IpcProcess {
  const emitter = new EventEmitter();
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
  };
  return proc;
}

describe('Bridge', () => {
  it('round trips a status request', async () => {
    const bridge = new Bridge(makeMockBackend());
    await expect(bridge.send('status')).resolves.toMatchObject({ ok: true });
  });
});

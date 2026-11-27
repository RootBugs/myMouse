export interface BridgeOptions {
  timeoutMs?: number;
}

export interface IpcProcess {
  stdin: { write(line: string): boolean };
  stdout: { on(event: 'data', cb: (chunk: Buffer | string) => void): void };
  onExit?: (cb: () => void) => void;
}

type Pending = {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  timer?: ReturnType<typeof setTimeout>;
};

export class Bridge {
  private nextId = 1;
  private buffer = '';
  private pending = new Map<number, Pending>();
  private notificationHandler: ((message: unknown) => void) | null = null;
  private exitHandlers: Array<() => void> = [];
  private closed = false;

  constructor(
    private proc: IpcProcess,
    private options: BridgeOptions = {},
  ) {
    this.proc.stdout.on('data', (chunk) => this.onData(chunk));
    this.proc.onExit?.(() => this.close('backend exited'));
  }

  send(method: string, params: unknown = {}): Promise<unknown> {
    if (this.closed) {
      return Promise.reject(new Error('backend exited'));
    }

    const id = this.nextId++;
    return new Promise<unknown>((resolve, reject) => {
      const pending: Pending = { resolve, reject };
      const timeoutMs = this.options.timeoutMs;
      if (timeoutMs !== undefined && timeoutMs > 0) {
        pending.timer = setTimeout(() => {
          if (this.pending.get(id) === pending) {
            this.pending.delete(id);
            reject(new Error(`IPC request '${method}' timed out after ${timeoutMs}ms`));
          }
        }, timeoutMs);
      }
      this.pending.set(id, pending);

      try {
        this.proc.stdin.write(JSON.stringify({ id, method, params }) + '\n');
      } catch (error) {
        this.pending.delete(id);
        this.clearTimer(pending);
        reject(error instanceof Error ? error : new Error(String(error)));
      }
    });
  }

  onNotification(cb: (message: unknown) => void): void {
    this.notificationHandler = cb;
  }

  onExit(cb: () => void): void {
    this.exitHandlers.push(cb);
  }

  close(reason = 'backend exited'): void {
    if (this.closed) return;
    this.closed = true;

    for (const [id, pending] of this.pending) {
      this.pending.delete(id);
      this.clearTimer(pending);
      pending.reject(new Error(reason));
    }
    for (const cb of this.exitHandlers) {
      cb();
    }
  }

  private clearTimer(pending: Pending): void {
    if (pending.timer !== undefined) {
      clearTimeout(pending.timer);
      pending.timer = undefined;
    }
  }

  private notify(message: unknown): void {
    this.notificationHandler?.(message);
  }

  private notifyMalformed(line: string): void {
    this.notify({ notification: 'malformedIpcMessage', line });
  }

  private isObject(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null;
  }

  private onData(chunk: Buffer | string): void {
    this.buffer += chunk.toString();
    const lines = this.buffer.split('\n');
    this.buffer = lines.pop() ?? '';
    for (const line of lines) {
      if (!line.trim()) continue;
      let message: unknown;
      try {
        message = JSON.parse(line);
      } catch {
        continue;
      }

      if (!this.isObject(message)) {
        this.notifyMalformed(line);
        continue;
      }

      const id = message.id;
      if (!('id' in message) || !Number.isInteger(id)) {
        if ('id' in message) {
          this.notifyMalformed(line);
        } else {
          this.notify(message);
        }
        continue;
      }

      const result = message.result;
      if (!('result' in message) || !this.isObject(result)) {
        this.notifyMalformed(line);
        continue;
      }

      const pending = this.pending.get(id as number);
      if (!pending) {
        this.notifyMalformed(line);
        continue;
      }
      this.pending.delete(id as number);
      this.clearTimer(pending);

      if (message.ok === false || result.ok === false) {
        const detail = result.error ?? message.error ?? 'IPC request failed';
        pending.reject(new Error(String(detail)));
      } else {
        pending.resolve(result);
      }
    }
  }
}

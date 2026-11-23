export interface IpcProcess {
  stdin: { write(line: string): boolean };
  stdout: { on(event: 'data', cb: (chunk: Buffer | string) => void): void };
}

type Pending = {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
};

export class Bridge {
  private nextId = 1;
  private buffer = '';
  private pending = new Map<number, Pending>();
  private notificationHandler: ((message: unknown) => void) | null = null;

  constructor(private proc: IpcProcess) {
    this.proc.stdout.on('data', (chunk) => this.onData(chunk));
  }

  send(method: string, params: unknown = {}): Promise<unknown> {
    const id = this.nextId++;
    return new Promise<unknown>((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.proc.stdin.write(JSON.stringify({ id, method, params }) + '\n');
    });
  }

  onNotification(cb: (message: unknown) => void): void {
    this.notificationHandler = cb;
  }

  private onData(chunk: Buffer | string): void {
    this.buffer += chunk.toString();
    const lines = this.buffer.split('\n');
    this.buffer = lines.pop() ?? '';
    for (const line of lines) {
      if (!line.trim()) continue;
      let message: any;
      try {
        message = JSON.parse(line);
      } catch {
        continue;
      }
      const pending = this.pending.get(message.id);
      if (pending) {
        this.pending.delete(message.id);
        pending.resolve(message.result ?? { ok: message.ok });
      } else if (this.notificationHandler) {
        this.notificationHandler(message);
      }
    }
  }
}

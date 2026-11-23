declare global {
  interface Window {
    mymouse: {
      status: () => Promise<{ ok: boolean; connected: boolean }>;
      devices: () => Promise<Array<{ name: string; id?: string }>>;
      pairingUri: () => Promise<string>;
      approvePairing: (code: string) => Promise<{ ok: boolean }>;
      setLayout: (layout: unknown) => Promise<{ ok: boolean }>;
    };
  }
}

export {};

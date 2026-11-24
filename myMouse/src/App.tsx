import React, { useCallback, useEffect, useState } from 'react';
import Dashboard from './components/Dashboard';
import DeviceLayout from './components/DeviceLayout';
import DeviceManagement from './components/DeviceManagement';
import FileTransfer from './components/FileTransfer';
import Settings from './components/Settings';
import type { AppSettings, TransferRecord } from './types';
import './styles.css';

type Section = 'Dashboard' | 'Device layout' | 'File transfer' | 'Device management' | 'Settings';

const sections: Section[] = ['Dashboard', 'Device layout', 'File transfer', 'Device management', 'Settings'];

const defaultSettings: AppSettings = {
  clipboard: { enabled: true, mode: 'global' },
  file_transfer: { auto_accept: false, save_dir: '', max_transfer_bytes: 100 * 1024 * 1024 },
  auto_reconnect: true,
  launch_at_start: false,
  quick_drop_enabled: true,
};

export default function App() {
  const [section, setSection] = useState<Section>('Dashboard');
  const [connected, setConnected] = useState(false);
  const [devices, setDevices] = useState<Array<{ name: string; id?: string }>>([]);
  const [settings, setSettings] = useState<AppSettings>(defaultSettings);
  const [transfers, setTransfers] = useState<TransferRecord[]>([]);

  const refresh = useCallback(async () => {
    try {
      const [status, deviceList, settingsResp, transferResp] = await Promise.all([
        window.mymouse.status(),
        window.mymouse.devices(),
        window.mymouse.getSettings(),
        window.mymouse.listTransfers(),
      ]);
      setConnected(status.connected);
      setDevices(deviceList);
      if (settingsResp?.settings) setSettings(settingsResp.settings);
      if (transferResp?.transfers) setTransfers(transferResp.transfers);
    } catch {
      // Backend may not be available in tests/dev; keep defaults.
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <div className="app-shell">
      <nav className="sidebar" aria-label="Main navigation">
        <div className="brand">myMouse</div>
        {sections.map((item) => (
          <button
            key={item}
            className={item === section ? 'nav-item active' : 'nav-item'}
            onClick={() => setSection(item)}
          >
            {item}
          </button>
        ))}
      </nav>
      <main className="content">
        {section === 'Dashboard' && <Dashboard connected={connected} devices={devices} transfers={transfers} settings={settings} />}
        {section === 'Device layout' && <DeviceLayout />}
        {section === 'File transfer' && <FileTransfer transfers={transfers} settings={settings} onRefresh={refresh} />}
        {section === 'Device management' && <DeviceManagement devices={devices} />}
        {section === 'Settings' && <Settings settings={settings} onChange={setSettings} onRefresh={refresh} />}
      </main>
    </div>
  );
}
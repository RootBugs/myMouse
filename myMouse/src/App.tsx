import React, { useEffect, useState } from 'react';
import Dashboard from './components/Dashboard';
import DeviceLayout from './components/DeviceLayout';
import DeviceManagement from './components/DeviceManagement';
import Settings from './components/Settings';
import './styles.css';

type Section = 'Dashboard' | 'Device layout' | 'File transfer' | 'Device management' | 'Settings';

const sections: Section[] = ['Dashboard', 'Device layout', 'File transfer', 'Device management', 'Settings'];

export default function App() {
  const [section, setSection] = useState<Section>('Dashboard');
  const [connected, setConnected] = useState(false);
  const [devices, setDevices] = useState<Array<{ name: string; id?: string }>>([]);

  useEffect(() => {
    window.mymouse.status().then((status) => setConnected(status.connected)).catch(() => undefined);
    window.mymouse.devices().then(setDevices).catch(() => undefined);
  }, []);

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
        {section === 'Dashboard' && <Dashboard connected={connected} devices={devices} />}
        {section === 'Device layout' && <DeviceLayout />}
        {section === 'File transfer' && (
          <section>
            <h1>File transfer</h1>
            <p className="hint">Clipboard and file transfer arrive in phase 2.</p>
          </section>
        )}
        {section === 'Device management' && <DeviceManagement devices={devices} />}
        {section === 'Settings' && <Settings />}
      </main>
    </div>
  );
}

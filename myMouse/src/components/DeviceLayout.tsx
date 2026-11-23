import React from 'react';

export default function DeviceLayout() {
  return (
    <section>
      <h1>Device layout</h1>
      <svg viewBox="0 0 600 360" className="layout-canvas" role="img" aria-label="Device adjacency layout">
        <rect x="40" y="120" width="220" height="120" rx="8" className="layout-node" />
        <text x="150" y="190" textAnchor="middle" className="layout-label">This PC</text>
        <line x1="260" y1="180" x2="340" y2="180" className="layout-edge" />
        <rect x="340" y="120" width="220" height="120" rx="8" className="layout-node" />
        <text x="450" y="190" textAnchor="middle" className="layout-label">Trusted peer</text>
      </svg>
      <p className="hint">Cursor crosses only configured edges. No wrap by default.</p>
    </section>
  );
}

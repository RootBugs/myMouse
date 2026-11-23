import React from 'react';

export default function Settings() {
  return (
    <section>
      <h1>Settings</h1>
      <label>Language <select defaultValue="en"><option value="en">English</option></select></label>
      <label><input type="checkbox" defaultChecked /> Auto reconnect</label>
      <label><input type="checkbox" /> Launch at Windows start (off until you enable it)</label>
      <label><input type="checkbox" defaultChecked /> Quick Drop zone</label>
      <p className="hint">Escape hotkey: Ctrl+Alt+Shift+M</p>
      <p className="hint">v1 cannot inject into UAC prompts, secure desktop, or locked sessions.</p>
    </section>
  );
}

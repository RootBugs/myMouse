# Production topology and handoff — decision record

Date: 2026-11-27

## Scope

This milestone makes the existing adjacency model operational in production:

1. Persist and load a validated layout in the backend config directory.
2. Make `setLayout` validate, store, and apply the layout instead of no-oping.
3. Detect real captured cursor crossings from `MouseMove` events on the active
   local node and hand off to exactly one connected peer.
4. Drive the React layout view from the backend layout state.

The work is autonomous under the existing milestone directive; there is no
separate approval gate for these design choices.

## Decisions

- Keep `shared/protocol` v2 unchanged. Layout is local backend/UI state, not a
  network protocol message. Crossings continue to use one existing input event
  and one pinned TLS connection to the single connected peer.
- Store topology in `layout.json` beside `device.json`, using serde. Loading a
  missing file yields an empty layout; a malformed or invalid file is reported
  as an error rather than silently replaced.
- Validate topology before persisting or applying it: node IDs and dimensions,
  unique `(device_id, monitor_index)` pairs, valid edge endpoints/sides, and no
  self-edges. Invalid input is rejected through the existing RPC error path.
- Keep mDNS fingerprints as untrusted hints. Discovery and persistence never
  grant trust or create a connection; production handoff remains gated on an
  already connected, capability-authorized peer.
- Derive handoff from the existing `Layout::cross()` math. The local node is
  selected by this install's persistent device ID and the current primary
  monitor index. A crossing is emitted only when a configured edge exists and a
  single peer is active; no peer means no production capture gate and no
  handoff.
- Preserve one-way control semantics: when control crosses away from the local
  node, locally captured input is suppressed until control returns. The
  emergency stop/pause path remains authoritative.
- Make `DeviceLayout.tsx` consume layout data from the backend RPC state rather
  than hardcoded demonstration nodes. UI changes must not invent a second
  transport or protocol.

## Non-goals

- No protocol version bump and no new wire message.
- No multi-peer routing or simultaneous control of two targets.
- No DPI/multi-monitor coordinate compensation beyond the existing proportional
  scaling in `Layout::cross()`.
- No physical PC/tablet verification, APK build, macOS build, or signing.
- No change to trust, pairing, or discovery semantics.

## Verification strategy

TDD order: failing test first, then minimal implementation, then full suite.

- Backend unit tests: layout serde round-trip, invalid layout rejection,
  persisted load/save, `setLayout` storage/runtime update, and real
  `MouseMove` crossing detection.
- Frontend tests: layout view renders backend-provided nodes/edges and handles
  empty/error state.
- Release checks: backend/protocol tests, npm tests/build, smoke harness,
  packaging, installer verification, clean diff, and push to `origin/master`.

Runtime evidence will still be labelled harness-verified, not physically
verified, because no second physical PC/tablet is available on this host.

## Implemented outcome

- `layout.json` uses camelCase `nodes`/`edges` JSON. Missing file → empty
  layout; malformed/invalid file → startup error. `setLayout` validates before
  atomic save and before replacing the shared runtime layout; `getLayout`
  returns the same state and `layoutChanged` is emitted after a successful set.
- The production `InputBridge` is topology-aware. Before a crossing it returns
  `false` to the Windows hook so local input is unchanged. On a `MouseMove` at a
  configured local edge it requires a connected gate and an active peer ID
  equal to the edge target, sends exactly one proportionally scaled move, and
  records that peer as the handoff target. Later events forward only to that
  peer; peer change/disconnect, Escape, and emergency pause clear the handoff.
- `DeviceLayout.tsx` renders backend nodes/edges and an explicit empty state.
  No v2 wire message, protocol version, trust rule, or broadcast path changed.
- Verified with backend/protocol unit tests, frontend tests, build, two-instance
  pinned-TLS smoke harness, unsigned installer build, and artifact verification.
  Live physical cursor crossing, multi-monitor/DPI behaviour, and return-to-local
  re-entry remain untested and are not claimed.

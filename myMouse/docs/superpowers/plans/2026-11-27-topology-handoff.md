# Topology and production handoff plan

## Goal

Make the adjacency layout operational in production without changing v2 wire
protocol: persist it, validate it, expose it to the UI, and trigger a real
one-peer handoff from Windows `MouseMove` edge crossings.

## Approach

- `backend/src/layout.rs`: derive serde, add camelCase JSON shape, validation,
  atomic save/load. Reject zero dimensions, duplicate node keys, duplicate
  source/side edges, unknown endpoints, self-edges, and oversized layouts.
- `backend/src/main.rs`: store `Arc<RwLock<Layout>>` in `AppState`; load from
  `layout.json` at startup; implement `setLayout` via a shared validation/
  persistence helper; add `getLayout`; emit `layoutChanged` notifications.
- `backend/src/input.rs`: add a topology-aware constructor. Before handoff,
  ordinary captured input stays local. A `MouseMove` at a configured local
  edge sends exactly one scaled move to the connected peer and activates
  forwarding to that peer until Escape/local stop. Keep existing constructors
  unchanged so deterministic test mode remains stable.
- `electron/preload.ts` and `src/types.d.ts`: expose `getLayout` with typed
  layout shapes.
- `src/App.tsx` and `src/components/DeviceLayout.tsx`: fetch layout with the
  normal refresh cycle and render real nodes/edges, with an explicit empty
  state.
- Docs: update README, limitations, phase audit, and the decision record after
  verification.

## TDD tasks

1. Red: layout serde round-trip and validation tests. Green: serde + validation.
2. Red: layout file save/load and `setLayout` helper tests. Green: AppState
   layout storage, atomic persistence, RPC wiring, and `getLayout`.
3. Red: topology-aware `InputBridge` tests for no forwarding before crossing,
   one scaled crossing event, and forwarding after crossing. Green: bridge
   handoff state and production wiring.
4. Red: `DeviceLayout` renders supplied topology and App fetches `getLayout`.
   Green: preload/types/App/component changes.
5. Full verification: backend tests, protocol tests, npm tests, build, smoke,
   dist, installer verification. Update docs honestly and push `origin/master`.

## Guardrails

- No protocol version bump or new wire message; layout RPCs are local IPC.
- Discovery/persistence never grants trust; handoff requires a connected peer.
- Outbound remains one peer only.
- No claim of physical PC/tablet verification, Android build, macOS build,
  signing, DPI compensation, or remote-to-local re-entry.

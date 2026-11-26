# myMouse engineering roadmap

Last updated: 2026-11-27. This ranks gaps found by reading the source, tests
and the existing Phase 1 audit. Status labels are deliberately narrow:
**implemented**, **unit-tested**, **harness-tested**, **physically tested**, or
**not implemented**. A label never implies a stronger one.

No upstream product was benchmarked on this host. No second physical PC or
tablet was available, so no physical field behaviour was verified here.

## Ranked gaps

| Rank | Gap | Evidence | Status | Next action |
| --- | --- | --- | --- | --- |
| 1 | No reproducible performance baseline; latency, throughput and resource claims cannot be made | `phase1-audit.md` lists no performance commands; `package.json` has no benchmark script | not implemented | Add `scripts/benchmark.ps1` and publish measured two-process values with explicit labels |
| 2 | A peer that goes silent while input is held can leave injected keys or mouse buttons down on the target | `network.rs` has a 250-event/s limiter but no liveness timeout; `input.rs` tracks only target modifiers | not implemented | Reuse v2 `Status` as a heartbeat; release held injected input after a 3 s silence |
| 3 | No live two-PC/tablet validation of discovery, pairing, handoff, mDNS, Wi-Fi interruption or real multi-monitor geometry | Phase 1 audit and limitations: no second physical machine on this host | physically tested: none | Obtain two physical machines; run the documented pairing and handoff checklist before any field claim |
| 4 | Per-monitor DPI compensation and robust mixed-DPI coordinate mapping | `input.rs` uses proportional virtual-screen scaling only; limitations explicitly call this incomplete | not implemented | Define scaling rules after physical multi-monitor measurements; keep proportional fallback |
| 5 | Return-to-local re-entry when the cursor crosses back from the peer | Audit lists this as open; the current authoritative paths are Escape/local stop and emergency pause | not implemented | Design a v2-compatible control path, then test on two physical machines |
| 6 | Replay protection for input frames | Protocol v2 has version, bounds and pinning but no nonce or timestamp on `InputEvent` | not implemented | Add bounded sequence tracking without a protocol bump if possible; otherwise plan an explicit versioned migration |
| 7 | Automatic session recovery beyond one reconnect attempt | Test mode retries a pinned connection for ~10 s; production has discovery-backed auto-connect but no field-proofed recovery SLA | harness-tested | Measure reconnect time first, then improve backoff and state restore if data justifies it |
| 8 | Multi-peer routing and topology beyond one connected peer | Runtime selects exactly one peer; this is a deliberate earlier milestone scope decision | not implemented | Keep out of the current increment; prioritise only after ranks 1–5 are resolved |
| 9 | Full drag-and-drop topology editor | `DeviceLayout.tsx` renders backend state; no editor exists | not implemented | Add editing after physical topology validation defines the coordinate model |
| 10 | Image clipboard end-to-end verification | Protocol and manager carry `ClipboardKind::Image`; only text is covered by the two-instance integration test | unit-tested | Add a two-process image clipboard test before claiming universal clipboard support |
| 11 | Android APK and macOS build verification | `apps/android/` and `apps/mac/` are source-only; no SDK/NDK/Gradle or Swift toolchain here | not implemented | Build and exercise on a machine with the respective toolchains |
| 12 | Signed Windows release | `npm run dist` produces unsigned artifacts; no certificate supplied | not implemented | Add signing only when a certificate is available; keep SmartScreen warning documented until then |

## What this increment changes

- Rank 1 is now measured and harness-tested: `npm run benchmark` writes
  `docs/benchmarks.md` with two-process loopback values and explicitly labels
  reconnect as unmeasured.
- Discovered and fixed during benchmarking: file transfers larger than one
  256 KiB chunk stalled after the first chunk. The regression test
  `accept_sends_every_chunk_for_large_file` is unit-tested, and the completed
  1 MiB transfer is harness-tested through the benchmark.
- Rank 2 becomes implemented, unit-tested and wired once the liveness watchdog
  lands. Held-input release remains hardware-unverified until a physical two-PC
  session exercises `SendInput`.
- Ranks 3–12 stay open. They are listed so that scope is visible, not as
  promises of delivery.

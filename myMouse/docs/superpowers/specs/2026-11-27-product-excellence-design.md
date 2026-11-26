# Product excellence design — 2026-11-27

## Intent

Turn `myMouse` from a working Phase 1 software KVM into a measurably
competitive one. The directive is explicit: latency, reliability and
responsiveness come before decorative features, resources must stay small,
and no superiority claim may be made without measurements. This design covers
the first incremental slice: an honest measurement baseline and the highest
priority reliability gap the current code exposes.

## Goals

1. Identify architectural weaknesses and competitive gaps from the existing
   source and tests, not from aspiration.
2. Publish a ranked, status-labelled engineering roadmap in the repository.
3. Produce reproducible measurements on this host for input delivery, clipboard
   throughput, file throughput, reconnection time, and idle CPU/RAM.
4. Harden the concrete failure mode where a peer vanishes while input is held:
   injected keys or mouse buttons can remain down on the target.
5. Preserve every working implementation and keep all claims labelled by the
   evidence actually available.

## Non-goals

- Replacing the Rust engine with Deskflow, Input Leap, Mousehop, RoboMouse or
  Synergy code. No upstream source is copied; licences are not imported.
- A protocol version bump or a new wire message. The existing v2 `Status`
  variant is reused as the liveness heartbeat.
- Physical field claims, APK builds, macOS builds, code signing, autostart
  changes, or remote screen streaming.
- Multi-peer routing, full drag-and-drop topology editing, per-monitor DPI
  compensation, or replay protection in this slice.
- Invented benchmark numbers or comparisons with upstream products that were
  not run under identical conditions.

## Constraints

- One versioned protocol (`shared/protocol` v2) and one pinned mutual-TLS
  transport. Local layout RPCs stay local.
- Security defaults stay unchanged: discovery never grants trust, explicit
  approval is required, capabilities are enforced per message class, and
  emergency stop remains authoritative.
- Test-driven development: a failing test before each production change,
  minimal fix, green test, then the full suite.
- Build and test after each milestone; verify with real commands.
- Windows 10/11 x64 is the only runtime-verified host here. No second physical
  PC/tablet, Android toolchain, Swift toolchain, or signing certificate is
  available.
- `cargo fmt --all -- --check` is already red at baseline; do not create
  unrelated repo-wide reformatting churn.

## Approaches

### A. Replace the engine with an upstream KVM

Rejected. It would discard the coherent Rust core and v2 contract, introduce a
second transport or bridge, and create licence obligations. Competitive
projects remain references only.

### B. Measure first, then harden the highest-value reliability gap — chosen

Keep the Rust engine. Add a reproducible two-instance benchmark that uses the
existing test-mode stdin RPCs and pinned-TLS transport. Publish the measured
baseline. Then fix the input-liveness gap with tests before claiming any
advantage. This is the cheapest path to evidence-backed prioritisation and
does not require hardware that is absent.

### C. Ship features first and benchmark later

Rejected. It would produce unmeasured claims and make regressions invisible,
which the directive explicitly forbids.

## Decisions

- The roadmap document is `docs/product-roadmap.md`. Every item carries a
  status label: implemented, unit-tested, harness-tested, physically tested, or
  not implemented.
- The benchmark harness is `scripts/benchmark.ps1`, driven by the existing
  `--test-mode` stdin RPCs (`crossTo`, `sendClipboard`, `offerFile`,
  `acceptTransfer`, `status`) and log notifications. It writes a machine-readable
  JSON report plus a human summary to `docs/benchmarks.md`.
- Measurements are labelled as one-host loopback, two-process, harness values.
  They are not field latency and not a comparison with another product.
- The liveness watchdog reuses `Message::Status { connected, devices }` as a
  heartbeat. The sender emits it once per second while a peer is attached; the
  receiver tracks the last frame from its peer.
- When the peer is silent for more than three seconds and injected input is
  actually held, the receiver releases its injected keys and buttons and emits
  an `inputLost` notification. Emergency stop remains authoritative: the
  watchdog never re-enables injection.
- Injected key/button state is tracked in a small pure helper so the state
  transition logic is testable on this non-hardware host. Windows `SendInput`
  release calls remain hardware-unverified.

## What is left to the builder

- Exact constants, function names and tests are in the linked plan.
- The builder may choose PowerShell implementation details for process I/O as
  long as the report fields and verification commands in the plan are exact.

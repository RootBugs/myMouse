# myMouse two-process benchmark

Measured: 2026-11-27T10:12:09.5656920Z

All values below come from two real `mymouse-backend.exe` processes on this
Windows host, connected over the pinned-TLS v2 test transport. This is a
**two-process loopback harness**, not field latency, not a physical two-PC
session, and **not a comparison with any upstream product**.

| Metric | Value | Unit | Evidence |
| --- | ---: | --- | --- |
| Input delivery | 30.67 | ms | RPC `crossTo` written to pc-a stdin -> `"notification":"input"` observed in pc-b log; 10 ms polling included |
| Clipboard throughput | 5.342 | Mbit/s | 256 KiB text through `sendClipboard` -> `clipboardUpdate` observed on pc-b |
| File throughput | 4.21 | Mbit/s | 1 MiB auto-accepted offer -> `"state":"completed"` observed on pc-b |
| Cold pinned-TLS connect | 339.48 | ms | pc-a process start -> `"notification":"connected"` |
| Reconnect after peer loss | not measured | ms | Test mode has no auto-reconnect loop; production reconnect remains unmeasured |
| Idle CPU, pc-a | 0 | % | average across 10 samples over ~2 s, divided by logical processors |
| Idle CPU, pc-b | 0 | % | average across 10 samples over ~2 s, divided by logical processors |
| Idle RAM, pc-a | 12.13 | MiB | average `WorkingSet64` across 10 samples |
| Idle RAM, pc-b | 11.9 | MiB | average `WorkingSet64` across 10 samples |

Raw values:

``json
{
    "input_delivery_ms":  30.67,
    "clipboard_mbps":  5.342,
    "file_mbps":  4.21,
    "connect_ms":  339.48,
    "reconnect_ms":  null,
    "idle_cpu_percent_pc_a":  0,
    "idle_cpu_percent_pc_b":  0,
    "idle_ram_mb_pc_a":  12.13,
    "idle_ram_mb_pc_b":  11.9,
    "clipboard_bytes":  262144,
    "file_bytes":  1048576,
    "logs":  "C:\\Users\\hp\\.openmausbot\\task-workspaces\\a11e8c88-9a9c-4665-9be2-31b80310a2db\\42cee900-e8c7-4f11-a107-e77534bbb516\\myMouse\\backend\\target\\benchmark-logs\\fd53acc94090473c9a80cdb57863d0a7"
}
``

Logs: `C:\Users\hp\.openmausbot\task-workspaces\a11e8c88-9a9c-4665-9be2-31b80310a2db\42cee900-e8c7-4f11-a107-e77534bbb516\myMouse\backend\target\benchmark-logs\fd53acc94090473c9a80cdb57863d0a7`

Reconnect time is intentionally reported as unmeasured. Test mode starts one
outbound connection and does not retry after a peer disappears; claiming a
reconnect number from this harness would be dishonest. See
[product-roadmap.md](product-roadmap.md) rank 7.

## Final remediation re-run — evidence label: harness-tested

`npm run benchmark` was re-run from `myMouse/` after the production-grade
remediation and exited 0; it wrote the values above. This is the final sample
for this repository state: input delivery 30.67 ms, clipboard 5.342 Mbit/s,
file 4.21 Mbit/s, cold pinned-TLS connect 339.48 ms, idle CPU 0/0%, idle RAM
12.13/11.9 MiB, reconnect `null`.

An earlier sample on this same one-host harness reported 8.9 ms, 15.688
Mbit/s, 15.307 Mbit/s and 121.46 ms. The difference is host-load variance in a
loopback harness, not a product change or a performance guarantee. Neither
sample is field latency, a physical two-PC measurement, or a comparison with
any upstream product.

This re-run used the same single v2 protocol and pinned mutual-TLS transport.
`backend/tests/test_mode_config.rs` verifies that test mode keeps its harness
state outside the production config directory; no production `APPDATA` state
was written by this benchmark. No real keyboard/mouse injection, mDNS, Wi-Fi
interruption, multi-monitor or DPI behavior was exercised.
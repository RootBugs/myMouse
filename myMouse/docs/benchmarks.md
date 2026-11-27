# myMouse two-process benchmark

Measured: 2026-11-27T05:07:58.7412130Z

All values below come from two real `mymouse-backend.exe` processes on this
Windows host, connected over the pinned-TLS v2 test transport. This is a
**two-process loopback harness**, not field latency, not a physical two-PC
session, and **not a comparison with any upstream product**.

| Metric | Value | Unit | Evidence |
| --- | ---: | --- | --- |
| Input delivery | 8.9 | ms | RPC `crossTo` written to pc-a stdin -> `"notification":"input"` observed in pc-b log; 10 ms polling included |
| Clipboard throughput | 15.688 | Mbit/s | 256 KiB text through `sendClipboard` -> `clipboardUpdate` observed on pc-b |
| File throughput | 15.307 | Mbit/s | 1 MiB auto-accepted offer -> `"state":"completed"` observed on pc-b |
| Cold pinned-TLS connect | 121.46 | ms | pc-a process start -> `"notification":"connected"` |
| Reconnect after peer loss | not measured | ms | Test mode has no auto-reconnect loop; production reconnect remains unmeasured |
| Idle CPU, pc-a | 0 | % | average across 10 samples over ~2 s, divided by logical processors |
| Idle CPU, pc-b | 0 | % | average across 10 samples over ~2 s, divided by logical processors |
| Idle RAM, pc-a | 12.07 | MiB | average `WorkingSet64` across 10 samples |
| Idle RAM, pc-b | 11.81 | MiB | average `WorkingSet64` across 10 samples |

Raw values:

``json
{
    "input_delivery_ms":  8.9,
    "clipboard_mbps":  15.688,
    "file_mbps":  15.307,
    "connect_ms":  121.46,
    "reconnect_ms":  null,
    "idle_cpu_percent_pc_a":  0,
    "idle_cpu_percent_pc_b":  0,
    "idle_ram_mb_pc_a":  12.07,
    "idle_ram_mb_pc_b":  11.81,
    "clipboard_bytes":  262144,
    "file_bytes":  1048576,
    "logs":  "C:\\Users\\hp\\.openmausbot\\task-workspaces\\a11e8c88-9a9c-4665-9be2-31b80310a2db\\42cee900-e8c7-4f11-a107-e77534bbb516\\myMouse\\backend\\target\\benchmark-logs\\ccda62e053974b27a636a6e86356847b"
}
``

Logs: `C:\Users\hp\.openmausbot\task-workspaces\a11e8c88-9a9c-4665-9be2-31b80310a2db\42cee900-e8c7-4f11-a107-e77534bbb516\myMouse\backend\target\benchmark-logs\ccda62e053974b27a636a6e86356847b`

Reconnect time is intentionally reported as unmeasured. Test mode starts one
outbound connection and does not retry after a peer disappears; claiming a
reconnect number from this harness would be dishonest. See
[product-roadmap.md](product-roadmap.md) rank 7.
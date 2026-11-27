# myMouse two-process benchmark

Measured: 2026-11-27T01:52:20.6122212Z

All values below come from two real `mymouse-backend.exe` processes on this
Windows host, connected over the pinned-TLS v2 test transport. This is a
**two-process loopback harness**, not field latency, not a physical two-PC
session, and **not a comparison with any upstream product**.

| Metric | Value | Unit | Evidence |
| --- | ---: | --- | --- |
| Input delivery | 25.31 | ms | RPC `crossTo` written to pc-a stdin -> `"notification":"input"` observed in pc-b log; 10 ms polling included |
| Clipboard throughput | 7.019 | Mbit/s | 256 KiB text through `sendClipboard` -> `clipboardUpdate` observed on pc-b |
| File throughput | 6.966 | Mbit/s | 1 MiB auto-accepted offer -> `"state":"completed"` observed on pc-b |
| Cold pinned-TLS connect | 236.6 | ms | pc-a process start -> `"notification":"connected"` |
| Reconnect after peer loss | not measured | ms | Test mode has no auto-reconnect loop; production reconnect remains unmeasured |
| Idle CPU, pc-a | 0 | % | average across 10 samples over ~2 s, divided by logical processors |
| Idle CPU, pc-b | 0 | % | average across 10 samples over ~2 s, divided by logical processors |
| Idle RAM, pc-a | 12.12 | MiB | average `WorkingSet64` across 10 samples |
| Idle RAM, pc-b | 11.82 | MiB | average `WorkingSet64` across 10 samples |

Raw values:

``json
{
    "input_delivery_ms":  25.31,
    "clipboard_mbps":  7.019,
    "file_mbps":  6.966,
    "connect_ms":  236.6,
    "reconnect_ms":  null,
    "idle_cpu_percent_pc_a":  0,
    "idle_cpu_percent_pc_b":  0,
    "idle_ram_mb_pc_a":  12.12,
    "idle_ram_mb_pc_b":  11.82,
    "clipboard_bytes":  262144,
    "file_bytes":  1048576,
    "logs":  "C:\\Users\\hp\\.openmausbot\\task-workspaces\\a11e8c88-9a9c-4665-9be2-31b80310a2db\\42cee900-e8c7-4f11-a107-e77534bbb516\\myMouse\\backend\\target\\benchmark-logs\\40fb5ec30507443cbb8fc7c0614d6610"
}
``

Logs: `C:\Users\hp\.openmausbot\task-workspaces\a11e8c88-9a9c-4665-9be2-31b80310a2db\42cee900-e8c7-4f11-a107-e77534bbb516\myMouse\backend\target\benchmark-logs\40fb5ec30507443cbb8fc7c0614d6610`

Reconnect time is intentionally reported as unmeasured. Test mode starts one
outbound connection and does not retry after a peer disappears; claiming a
reconnect number from this harness would be dishonest. See
[product-roadmap.md](product-roadmap.md) rank 7.
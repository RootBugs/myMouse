import Foundation

/// Transport/protocol integration plan for the macOS target.
///
/// Preferred path: reuse the shared Rust core (`backend/` + `shared/protocol`)
/// through a C/UniFFI binding layer so mDNS discovery, pairing, pinned TLS,
/// message framing, and trust semantics stay identical across platforms.
///
/// Alternative path: a native Swift implementation of the same wire protocol.
/// This is only acceptable if it passes cross-platform interop tests against
/// the Rust implementation and keeps certificate pinning semantics identical.
public struct TransportPlan {
    public static let preferredPath = "Shared Rust core via UniFFI/C FFI"
    public static let fallbackPath = "Native Swift implementation with protocol interop tests"

    public static let requiredCapabilities: [String] = [
        "mDNS discovery of _mymouse._tcp peers",
        "QR / 6-digit pairing handshake",
        "Pinned TLS 1.3 per pair (no TOFU after trust established)",
        "Versioned protocol decode/encode compatible with shared/protocol",
        "InputEvent forwarding and local injection",
        "Trust store revocation and capability toggles",
        "Reconnect with exponential backoff; no silent trust escalation",
    ]

    public static let integrationNotes: [String] = [
        "Expose a narrow FFI surface: start/stop node, pair, approve/reject, send input, revoke trust, status stream.",
        "Keep macOS-specific CGEvent and ScreenCaptureKit code in Swift; keep networking/crypto in Rust where possible.",
        "If Swift implements transport natively, reuse protocol field names and version header from shared/protocol/src/lib.rs.",
        "Do not open input forwarding before certificate fingerprint is pinned and user approval is recorded.",
    ]
}

public protocol PeerTransport: AnyObject {
    func connect() async throws
    func disconnect()
    func sendInput(_ event: InputEvent) async throws
}

public enum TransportError: Error, Sendable {
    case scaffoldOnly(String)
}

public final class ScaffoldTransport: PeerTransport {
    public init() {}

    public func connect() async throws {
        throw TransportError.scaffoldOnly(
            "Transport is scaffolded. Wire Rust core FFI or implement Swift transport before pairing over LAN."
        )
    }

    public func disconnect() {}

    public func sendInput(_ event: InputEvent) async throws {
        _ = event
        throw TransportError.scaffoldOnly("Input forwarding requires an active pinned transport.")
    }
}

import Foundation

public enum PairingState: String, Sendable {
    case idle
    case showingCode
    case awaitingApproval
    case trusted
    case rejected
}

public struct PairingSession: Equatable, Sendable {
    public let code: String
    public let qrPayload: String
    public let expiresAt: Date

    public init(code: String, qrPayload: String, expiresAt: Date) {
        self.code = code
        self.qrPayload = qrPayload
        self.expiresAt = expiresAt
    }
}

/// Pairing UI/domain scaffold. The real flow must mirror the Windows design:
/// host displays QR/6-digit code, joiner confirms, host approves, and the pair
/// pins a per-pair TLS certificate fingerprint before input forwarding.
public final class PairingController {
    public private(set) var state: PairingState = .idle
    public private(set) var session: PairingSession?

    public init() {}

    public func beginLocalSession(deviceID: String, deviceName: String) -> PairingSession {
        let code = Self.generateCode()
        let payload = "mymouse://pair?device=\(deviceID)&name=\(deviceName)&code=\(code)"
        let session = PairingSession(
            code: code,
            qrPayload: payload,
            expiresAt: Date().addingTimeInterval(300)
        )
        self.session = session
        self.state = .showingCode
        return session
    }

    public func approve() {
        state = .trusted
    }

    public func reject() {
        state = .rejected
    }

    public func reset() {
        state = .idle
        session = nil
    }

    static func generateCode() -> String {
        String(format: "%06d", Int.random(in: 0...999_999))
    }
}

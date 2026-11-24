import CryptoKit
import Foundation

public struct TrustedPeer: Codable, Equatable, Sendable {
    public let deviceID: String
    public let name: String
    public let certificateFingerprint: String
    public var capabilities: [String]
    public let pairedAt: Date

    public init(
        deviceID: String,
        name: String,
        certificateFingerprint: String,
        capabilities: [String],
        pairedAt: Date = Date()
    ) {
        self.deviceID = deviceID
        self.name = name
        self.certificateFingerprint = certificateFingerprint
        self.capabilities = capabilities
        self.pairedAt = pairedAt
    }
}

/// File-backed trust store for the Mac scaffold.
///
/// The long-term goal is parity with the Rust backend trust model (per-pair
/// pinned certificate fingerprint, per-capability grants, revocation). This
/// scaffold keeps data local and readable for early macOS development.
public final class TrustStore {
    private let fileURL: URL
    private let encoder: JSONEncoder
    private let decoder: JSONDecoder

    public init(fileURL: URL? = nil) {
        if let fileURL {
            self.fileURL = fileURL
        } else {
            let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
            self.fileURL = base
                .appendingPathComponent("myMouse", isDirectory: true)
                .appendingPathComponent("trusted-peers.json")
        }
        encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        encoder.dateEncodingStrategy = .iso8601
        decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
    }

    public func load() throws -> [TrustedPeer] {
        guard FileManager.default.fileExists(atPath: fileURL.path) else { return [] }
        let data = try Data(contentsOf: fileURL)
        return try decoder.decode([TrustedPeer].self, from: data)
    }

    public func save(_ peers: [TrustedPeer]) throws {
        let directory = fileURL.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let data = try encoder.encode(peers)
        try data.write(to: fileURL, options: .atomic)
    }

    public func upsert(_ peer: TrustedPeer) throws {
        var peers = try load()
        peers.removeAll { $0.deviceID == peer.deviceID }
        peers.append(peer)
        try save(peers)
    }

    public func revoke(deviceID: String) throws {
        var peers = try load()
        peers.removeAll { $0.deviceID == deviceID }
        try save(peers)
    }

    public static func fingerprint(certificateDER: Data) -> String {
        let digest = SHA256.hash(data: certificateDER)
        return digest.map { String(format: "%02x", $0) }.joined()
    }
}

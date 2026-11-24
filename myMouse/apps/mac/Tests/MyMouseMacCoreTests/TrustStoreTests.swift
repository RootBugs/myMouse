import XCTest
@testable import MyMouseMacCore

final class TrustStoreTests: XCTestCase {
    func testRoundTrip() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        let url = dir.appendingPathComponent("trusted-peers.json")
        let store = TrustStore(fileURL: url)
        let peer = TrustedPeer(deviceID: "id1", name: "PC", certificateFingerprint: "abc", capabilities: ["input"])
        try store.upsert(peer)
        let loaded = try store.load()
        XCTAssertEqual(loaded, [peer])
        try store.revoke(deviceID: "id1")
        XCTAssertTrue(try store.load().isEmpty)
    }

    func testFingerprintIsStable() {
        let fp = TrustStore.fingerprint(certificateDER: Data([1, 2, 3]))
        XCTAssertEqual(fp.count, 64)
        XCTAssertEqual(fp, TrustStore.fingerprint(certificateDER: Data([1, 2, 3])))
    }
}

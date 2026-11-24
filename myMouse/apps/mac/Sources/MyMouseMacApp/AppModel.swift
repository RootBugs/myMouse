import Foundation
import MyMouseMacCore

@MainActor
final class AppModel: ObservableObject {
    @Published var pairingCode: String = "------"
    @Published var pairingState: PairingState = .idle
    @Published var trustedPeers: [TrustedPeer] = []
    @Published var statusMessage: String = "Scaffold only. Run on macOS to wire permissions and transport."

    let permissions = PermissionCoordinator()
    private let pairing = PairingController()
    private let trustStore = TrustStore()

    func refresh() {
        permissions.refresh()
        trustedPeers = (try? trustStore.load()) ?? []
    }

    func beginPairing() {
        let session = pairing.beginLocalSession(deviceID: UUID().uuidString, deviceName: Host.current().localizedName ?? "Mac")
        pairingCode = session.code
        pairingState = pairing.state
        statusMessage = "Pairing session created. QR/transport wiring is scaffolded."
    }

    func approvePairing() {
        pairing.approve()
        pairingState = pairing.state
        let peer = TrustedPeer(
            deviceID: "scaffold-peer",
            name: "Pending peer",
            certificateFingerprint: "unwired-scaffold",
            capabilities: ["input"]
        )
        try? trustStore.upsert(peer)
        trustedPeers = (try? trustStore.load()) ?? []
        statusMessage = "Approval recorded locally. Real approval must pin peer TLS certificate."
    }

    func rejectPairing() {
        pairing.reject()
        pairingState = pairing.state
        statusMessage = "Pairing rejected."
    }
}

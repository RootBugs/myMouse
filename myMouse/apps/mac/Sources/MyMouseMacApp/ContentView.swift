import SwiftUI
import MyMouseMacCore

struct ContentView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        NavigationSplitView {
            List {
                Label("Overview", systemImage: "display")
                Label("Pairing", systemImage: "qrcode")
                Label("Permissions", systemImage: "lock.shield")
                Label("Trusted devices", systemImage: "checkmark.shield")
            }
            .navigationTitle("myMouse")
        } detail: {
            ScrollView {
                VStack(alignment: .leading, spacing: 24) {
                    overview
                    pairing
                    permissions
                    trustedDevices
                    footer
                }
                .padding(24)
            }
        }
        .onAppear { model.refresh() }
    }

    private var overview: some View {
        GroupBox("Phase 4 Mac scaffold") {
            VStack(alignment: .leading, spacing: 8) {
                Text("This is a macOS Apple Silicon target/helper scaffold.")
                Text("Real input injection, ScreenCaptureKit capture, and pinned transport are not wired on a Windows host.")
                    .foregroundStyle(.secondary)
                Text(model.statusMessage)
                    .font(.callout)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var pairing: some View {
        GroupBox("Pairing") {
            VStack(alignment: .leading, spacing: 12) {
                Text("Code: \(model.pairingCode)")
                    .font(.system(.title2, design: .monospaced))
                Text("State: \(model.pairingState.rawValue)")
                    .foregroundStyle(.secondary)
                HStack {
                    Button("Start pairing session", action: model.beginPairing)
                    Button("Approve", action: model.approvePairing)
                    Button("Reject", action: model.rejectPairing)
                }
                Text("Production flow: host shows QR/6-digit code, joiner confirms, host approves, certificate fingerprint is pinned before forwarding.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var permissions: some View {
        GroupBox("Permissions") {
            VStack(alignment: .leading, spacing: 10) {
                permissionRow(.accessibility, label: "Accessibility", detail: "Required for CGEvent input injection.")
                permissionRow(.inputMonitoring, label: "Input Monitoring", detail: "Required for observing input events; macOS may prompt on first capture.")
                permissionRow(.screenRecording, label: "Screen Recording", detail: "Required for ScreenCaptureKit display capture.")
                Button("Refresh") { model.permissions.refresh() }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func permissionRow(_ kind: PermissionKind, label: String, detail: String) -> some View {
        HStack(alignment: .top) {
            VStack(alignment: .leading) {
                Text(label).font(.headline)
                Text(detail).font(.footnote).foregroundStyle(.secondary)
            }
            Spacer()
            Text(model.permissions.state(for: kind).rawValue)
                .font(.caption)
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(.quaternary)
                .clipShape(Capsule())
            Button("Request/Open Settings") { model.permissions.request(kind) }
        }
    }

    private var trustedDevices: some View {
        GroupBox("Trusted devices") {
            if model.trustedPeers.isEmpty {
                Text("No trusted peers stored.").foregroundStyle(.secondary)
            } else {
                ForEach(model.trustedPeers, id: \.deviceID) { peer in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(peer.name).font(.headline)
                        Text(peer.deviceID).font(.caption).foregroundStyle(.secondary)
                        Text("Fingerprint: \(peer.certificateFingerprint)").font(.caption2)
                    }
                    Divider()
                }
            }
        }
    }

    private var footer: some View {
        GroupBox("Integration plan") {
            VStack(alignment: .leading, spacing: 6) {
                ForEach(TransportPlan.integrationNotes, id: \.self) { note in
                    Text("• \(note)")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

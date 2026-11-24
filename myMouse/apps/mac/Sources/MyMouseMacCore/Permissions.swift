import AppKit
import ApplicationServices
import Combine
import CoreGraphics
import Foundation

public enum PermissionKind: String, CaseIterable, Sendable {
    case accessibility
    case inputMonitoring
    case screenRecording
}

/// `.unknown` is used where macOS exposes no stable public query API.
public enum PermissionState: String, Sendable {
    case granted
    case denied
    case unknown
}

@MainActor
public final class PermissionCoordinator: ObservableObject {
    @Published public private(set) var accessibility: PermissionState = .unknown
    @Published public private(set) var inputMonitoring: PermissionState = .unknown
    @Published public private(set) var screenRecording: PermissionState = .unknown

    public init() {}

    public func refresh() {
        accessibility = AXIsProcessTrusted() ? .granted : .denied
        // Input Monitoring and Screen Recording have no public query API that
        // is reliable across macOS releases; the UI shows "check manually".
        inputMonitoring = .unknown
        screenRecording = .unknown
    }

    public func state(for kind: PermissionKind) -> PermissionState {
        switch kind {
        case .accessibility: return accessibility
        case .inputMonitoring: return inputMonitoring
        case .screenRecording: return screenRecording
        }
    }

    /// Accessibility can show the system prompt. The other two permissions
    /// must be toggled by the user in System Settings; we only open the pane.
    public func request(_ kind: PermissionKind) {
        switch kind {
        case .accessibility:
            let options = [kAXTrustedCheckOptionPrompt: true] as CFDictionary
            _ = AXIsProcessTrustedWithOptions(options)
            refresh()
        case .inputMonitoring, .screenRecording:
            openSettings(for: kind)
        }
    }

    public func openSettings(for kind: PermissionKind) {
        guard let url = URL(string: kind.settingsURL) else { return }
        NSWorkspace.shared.open(url)
    }
}

private extension PermissionKind {
    var settingsURL: String {
        switch self {
        case .accessibility:
            return "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        case .inputMonitoring:
            return "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
        case .screenRecording:
            return "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
        }
    }
}

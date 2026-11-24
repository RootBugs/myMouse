import CoreGraphics
import Foundation

public struct CapturedFrame: Sendable {
    public let width: Int
    public let height: Int
    public let bytesPerRow: Int
    public let pixelFormat: String
    public let data: Data

    public init(width: Int, height: Int, bytesPerRow: Int, pixelFormat: String, data: Data) {
        self.width = width
        self.height = height
        self.bytesPerRow = bytesPerRow
        self.pixelFormat = pixelFormat
        self.data = data
    }
}

/// Screen capture boundary. The real implementation is planned around
/// ScreenCaptureKit: request shareable content, pick a display, create an
/// SCStream, and forward SCStreamFrameOutput frames to a consumer.
///
/// CGDisplayCreateImage is intentionally not used as the production path here;
/// ScreenCaptureKit is the modern, permission-aware capture API for macOS 13+.
public protocol ScreenCapturing: AnyObject {
    var isCapturing: Bool { get }
    func start(onFrame: @escaping (CapturedFrame) -> Void) async throws
    func stop()
}

public enum ScreenCaptureError: Error, Sendable {
    case permissionMissing
    case notImplemented(String)
}

public final class ScreenCaptureKitStub: ScreenCapturing {
    public private(set) var isCapturing = false

    public init() {}

    public func start(onFrame: @escaping (CapturedFrame) -> Void) async throws {
        _ = onFrame
        throw ScreenCaptureError.notImplemented(
            "ScreenCaptureKit capture is scaffolded but not wired on this Windows host. " +
            "Implement SCShareableContent + SCStream on macOS and gate it behind Screen Recording permission."
        )
    }

    public func stop() {
        isCapturing = false
    }
}

public struct ScreenCapturePlan {
    public static let notes: [String] = [
        "Use SCShareableContent.current to enumerate displays after Screen Recording permission is granted.",
        "Create SCStreamConfiguration with pixel format BGRA and a bounded frame rate for remote viewing.",
        "Implement SCStreamOutput to convert CMSampleBuffer frames into CapturedFrame or encode H.264 for streaming.",
        "Stop the stream on app quit, permission loss, or when no remote viewer is connected.",
    ]
}

import CoreGraphics
import Foundation

/// Typed form of `CapturedEvent` from `backend/src/input.rs` after decoding
/// the version-1 bincode payload.
public enum InputEvent: Equatable, Sendable {
    case key(key: String, ctrl: Bool, alt: Bool, shift: Bool)
    case hotkey(String)
    case mouseMove(x: Int32, y: Int32)
}

/// Converts host virtual-screen coordinates into a target display rectangle.
public struct DisplayMapper {
    public let sourceWidth: Int32
    public let sourceHeight: Int32
    public let destination: CGRect

    public init(sourceWidth: Int32, sourceHeight: Int32, destination: CGRect) {
        self.sourceWidth = sourceWidth
        self.sourceHeight = sourceHeight
        self.destination = destination
    }

    public func point(x: Int32, y: Int32) -> CGPoint {
        let width = Double(max(1, sourceWidth))
        let height = Double(max(1, sourceHeight))
        let nx = min(1, max(0, Double(x) / width))
        let ny = min(1, max(0, Double(y) / height))
        return CGPoint(
            x: destination.minX + nx * destination.width,
            y: destination.minY + ny * destination.height
        )
    }
}

/// Posts events through CGEvent. Requires Accessibility permission at runtime.
/// Only key presses, the escape hotkey release, and mouse moves are wired;
/// buttons, scroll, drag, and IME handling are later work.
public final class InputInjector {
    private let source: CGEventSource?

    public init() {
        source = CGEventSource(stateID: .hidSystemState)
    }

    @discardableResult
    public func inject(_ event: InputEvent, mapper: DisplayMapper) -> Bool {
        switch event {
        case let .key(key, ctrl, alt, shift):
            guard let code = KeyCode.forName(key) else { return false }
            let flags = macFlags(ctrl: ctrl, alt: alt, shift: shift)
            postKey(code: code, flags: flags, keyDown: true)
            postKey(code: code, flags: flags, keyDown: false)
            return true

        case let .hotkey(name):
            if name == "Ctrl+Alt+Shift+M" {
                releaseModifiers()
                return true
            }
            guard let chord = KeyChord.parse(name),
                  let code = KeyCode.forName(chord.key) else { return false }
            postKey(code: code, flags: chord.flags, keyDown: true)
            postKey(code: code, flags: chord.flags, keyDown: false)
            return true

        case let .mouseMove(x, y):
            let location = mapper.point(x: x, y: y)
            guard let move = CGEvent(
                mouseEventSource: source,
                mouseType: .mouseMoved,
                mouseCursorPosition: location,
                mouseButton: .left
            ) else { return false }
            move.post(tap: .cghidEventTap)
            return true
        }
    }

    /// Windows Ctrl maps to Command and Windows Alt maps to Option on a Mac
    /// target, matching `backend::input::translate` for target_os == "mac".
    private func macFlags(ctrl: Bool, alt: Bool, shift: Bool) -> CGEventFlags {
        var flags: CGEventFlags = []
        if ctrl { flags.insert(.maskCommand) }
        if alt { flags.insert(.maskAlternate) }
        if shift { flags.insert(.maskShift) }
        return flags
    }

    private func postKey(code: CGKeyCode, flags: CGEventFlags, keyDown: Bool) {
        guard let event = CGEvent(
            keyboardEventSource: source,
            virtualKey: code,
            keyDown: keyDown
        ) else { return }
        event.flags = flags
        event.post(tap: .cghidEventTap)
    }

    private func releaseModifiers() {
        for code in [KeyCode.command, KeyCode.option, KeyCode.shift, KeyCode.control] {
            postKey(code: code, flags: [], keyDown: false)
        }
    }
}

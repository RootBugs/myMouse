import Carbon.HIToolbox
import CoreGraphics
import Foundation

/// A key plus explicit macOS modifiers. Parsed from strings such as `Cmd+C`.
public struct KeyChord: Equatable, Sendable {
    public var key: String
    public var command: Bool
    public var option: Bool
    public var shift: Bool
    public var control: Bool

    public init(
        key: String,
        command: Bool = false,
        option: Bool = false,
        shift: Bool = false,
        control: Bool = false
    ) {
        self.key = key
        self.command = command
        self.option = option
        self.shift = shift
        self.control = control
    }

    public static func parse(_ text: String) -> KeyChord? {
        let parts = text.split(separator: "+", omittingEmptySubsequences: false).map(String.init)
        guard let key = parts.last, !key.isEmpty else { return nil }
        var chord = KeyChord(key: key)
        for modifier in parts.dropLast() {
            switch modifier.lowercased() {
            case "cmd", "command": chord.command = true
            case "opt", "option", "alt": chord.option = true
            case "shift": chord.shift = true
            case "ctrl", "control": chord.control = true
            default: return nil
            }
        }
        return chord
    }

    public var flags: CGEventFlags {
        var value: CGEventFlags = []
        if command { value.insert(.maskCommand) }
        if option { value.insert(.maskAlternate) }
        if shift { value.insert(.maskShift) }
        if control { value.insert(.maskControl) }
        return value
    }
}

/// Maps the key names emitted by the Windows backend to macOS virtual key
/// codes. Unknown names are rejected rather than guessed.
public enum KeyCode {
    public static let command = CGKeyCode(kVK_Command)
    public static let option = CGKeyCode(kVK_Option)
    public static let shift = CGKeyCode(kVK_Shift)
    public static let control = CGKeyCode(kVK_Control)

    public static func forName(_ name: String) -> CGKeyCode? {
        names[name.trimmingCharacters(in: .whitespacesAndNewlines).uppercased()]
    }

    private static let names: [String: CGKeyCode] = [
        "A": CGKeyCode(kVK_ANSI_A), "B": CGKeyCode(kVK_ANSI_B),
        "C": CGKeyCode(kVK_ANSI_C), "D": CGKeyCode(kVK_ANSI_D),
        "E": CGKeyCode(kVK_ANSI_E), "F": CGKeyCode(kVK_ANSI_F),
        "G": CGKeyCode(kVK_ANSI_G), "H": CGKeyCode(kVK_ANSI_H),
        "I": CGKeyCode(kVK_ANSI_I), "J": CGKeyCode(kVK_ANSI_J),
        "K": CGKeyCode(kVK_ANSI_K), "L": CGKeyCode(kVK_ANSI_L),
        "M": CGKeyCode(kVK_ANSI_M), "N": CGKeyCode(kVK_ANSI_N),
        "O": CGKeyCode(kVK_ANSI_O), "P": CGKeyCode(kVK_ANSI_P),
        "Q": CGKeyCode(kVK_ANSI_Q), "R": CGKeyCode(kVK_ANSI_R),
        "S": CGKeyCode(kVK_ANSI_S), "T": CGKeyCode(kVK_ANSI_T),
        "U": CGKeyCode(kVK_ANSI_U), "V": CGKeyCode(kVK_ANSI_V),
        "W": CGKeyCode(kVK_ANSI_W), "X": CGKeyCode(kVK_ANSI_X),
        "Y": CGKeyCode(kVK_ANSI_Y), "Z": CGKeyCode(kVK_ANSI_Z),
        "0": CGKeyCode(kVK_ANSI_0), "1": CGKeyCode(kVK_ANSI_1),
        "2": CGKeyCode(kVK_ANSI_2), "3": CGKeyCode(kVK_ANSI_3),
        "4": CGKeyCode(kVK_ANSI_4), "5": CGKeyCode(kVK_ANSI_5),
        "6": CGKeyCode(kVK_ANSI_6), "7": CGKeyCode(kVK_ANSI_7),
        "8": CGKeyCode(kVK_ANSI_8), "9": CGKeyCode(kVK_ANSI_9),
        "ENTER": CGKeyCode(kVK_Return), "RETURN": CGKeyCode(kVK_Return),
        "TAB": CGKeyCode(kVK_Tab), "SPACE": CGKeyCode(kVK_Space),
        "BACKSPACE": CGKeyCode(kVK_Delete),
        "DELETE": CGKeyCode(kVK_ForwardDelete),
        "ESCAPE": CGKeyCode(kVK_Escape), "ESC": CGKeyCode(kVK_Escape),
        "LEFT": CGKeyCode(kVK_LeftArrow), "RIGHT": CGKeyCode(kVK_RightArrow),
        "UP": CGKeyCode(kVK_UpArrow), "DOWN": CGKeyCode(kVK_DownArrow),
    ]
}

use crate::layout::escape_hotkey;
use mymouse_protocol::Message;
use serde::de::Error as DeserializeError;
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// An input event observed on the host or received from a peer.
///
/// `Hotkey` keeps a `&'static str` in the public API; deserialization maps the
/// known wire strings back to static values so no untrusted string is leaked
/// into a long-lived allocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum CapturedEvent {
    Key {
        key: String,
        ctrl: bool,
        alt: bool,
        shift: bool,
    },
    Hotkey(&'static str),
    MouseMove { x: i32, y: i32 },
}

impl<'de> Deserialize<'de> for CapturedEvent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        enum WireEvent {
            Key {
                key: String,
                ctrl: bool,
                alt: bool,
                shift: bool,
            },
            Hotkey(String),
            MouseMove { x: i32, y: i32 },
        }

        match WireEvent::deserialize(deserializer)? {
            WireEvent::Key {
                key,
                ctrl,
                alt,
                shift,
            } => Ok(CapturedEvent::Key {
                key,
                ctrl,
                alt,
                shift,
            }),
            WireEvent::Hotkey(name) => {
                let known = match name.as_str() {
                    "Ctrl+Alt+Shift+M" => "Ctrl+Alt+Shift+M",
                    _ => return Err(D::Error::custom("unknown hotkey")),
                };
                Ok(CapturedEvent::Hotkey(known))
            }
            WireEvent::MouseMove { x, y } => Ok(CapturedEvent::MouseMove { x, y }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslatedEvent {
    pub key: String,
}

/// Translates modifier names for a target OS. Mouse moves and hotkeys are
/// control signals, not key combinations, so they are not translated.
pub fn translate(event: &CapturedEvent, target_os: &str) -> Option<TranslatedEvent> {
    let CapturedEvent::Key {
        key,
        ctrl,
        alt,
        shift,
    } = event
    else {
        return None;
    };

    let mut parts: Vec<&str> = Vec::new();
    if target_os == "mac" {
        if *ctrl {
            parts.push("Cmd");
        }
        if *alt {
            parts.push("Option");
        }
        if *shift {
            parts.push("Shift");
        }
    } else {
        if *ctrl {
            parts.push("Ctrl");
        }
        if *alt {
            parts.push("Alt");
        }
        if *shift {
            parts.push("Shift");
        }
    }
    parts.push(key.as_str());
    Some(TranslatedEvent {
        key: parts.join("+"),
    })
}

pub struct InputBridge {
    pub forwarding: bool,
    pub target: String,
    outgoing: Option<mpsc::UnboundedSender<Message>>,
}

impl Default for InputBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl InputBridge {
    pub fn new() -> Self {
        Self {
            forwarding: false,
            target: String::new(),
            outgoing: None,
        }
    }

    /// Test/transport-friendly constructor: captured events are emitted as
    /// `Message::InputEvent` on `outgoing` while forwarding is active.
    pub fn with_sender(target: &str, outgoing: mpsc::UnboundedSender<Message>) -> Self {
        Self {
            forwarding: false,
            target: target.to_string(),
            outgoing: Some(outgoing),
        }
    }

    /// Handle a locally captured event. When forwarding, the event is sent to
    /// the target and the caller (OS hook) should suppress local delivery.
    /// When not forwarding the OS keeps the event; remote events are applied
    /// through `inject`, which prevents local/remote loops.
    pub fn handle(&mut self, event: CapturedEvent) {
        if let CapturedEvent::Hotkey(name) = &event {
            if *name == escape_hotkey() {
                self.forwarding = false;
                Self::inject(&event);
                return;
            }
        }

        if !self.forwarding {
            return;
        }

        let Some(outgoing) = &self.outgoing else {
            return;
        };
        let payload = bincode::serialize(&event).unwrap_or_default();
        let _ = outgoing.send(Message::InputEvent {
            target: self.target.clone(),
            payload,
        });
    }

    /// Apply an event received from a peer on the local machine.
    pub fn inject(event: &CapturedEvent) {
        #[cfg(windows)]
        win::inject(event);
        #[cfg(not(windows))]
        let _ = event;
    }

    /// Install OS hooks (Windows only). The returned task owns the hook
    /// message loop and ends when the hooks are stopped.
    pub fn start(bridge: Arc<Mutex<InputBridge>>) -> JoinHandle<()> {
        #[cfg(windows)]
        {
            win::start_hooks(bridge)
        }
        #[cfg(not(windows))]
        {
            tokio::spawn(async move {
                let _ = bridge;
            })
        }
    }

    pub fn stop(&mut self) {
        self.forwarding = false;
        #[cfg(windows)]
        win::stop_hooks();
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::OnceLock;
    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MapVirtualKeyW, SendInput, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT,
        KEYEVENTF_KEYUP, MAPVK_VK_TO_CHAR, MAPVK_VK_TO_VSC, MOUSEEVENTF_ABSOLUTE,
        MOUSEEVENTF_MOVE, MOUSEINPUT, VIRTUAL_KEY, INPUT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, GetSystemMetrics, SetWindowsHookExW, TranslateMessage,
        UnhookWindowsHookEx, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT,
        SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
        WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_MOUSEMOVE, WM_SYSKEYDOWN,
        WM_SYSKEYUP,
    };

    const MOD_CTRL: u8 = 1;
    const MOD_ALT: u8 = 2;
    const MOD_SHIFT: u8 = 4;
    const VK_CONTROL: u32 = 0x11;
    const VK_MENU: u32 = 0x12;
    const VK_SHIFT: u32 = 0x10;
    const VK_M: u32 = 0x4d;

    static BRIDGE: OnceLock<Arc<Mutex<InputBridge>>> = OnceLock::new();
    // Store raw pointers: HHOOK is !Send, but only this module's hook thread
    // and the stop() call site touch the list.
    static HOOKS: Mutex<Vec<isize>> = Mutex::new(Vec::new());
    static MODIFIERS: AtomicU8 = AtomicU8::new(0);

    struct HookGuard(Vec<isize>);

    impl Drop for HookGuard {
        fn drop(&mut self) {
            for hook in self.0.drain(..) {
                unsafe {
                    let _ = UnhookWindowsHookEx(HHOOK(hook as *mut c_void));
                }
            }
        }
    }

    fn key_name(vk: u32) -> String {
        let mapped = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_CHAR) };
        let ch = char::from_u32(mapped & 0xffff)
            .filter(|c| !c.is_control())
            .unwrap_or('?');
        if ch.is_alphanumeric() {
            return ch.to_ascii_uppercase().to_string();
        }
        match vk {
            8 => "Backspace".into(),
            9 => "Tab".into(),
            13 => "Enter".into(),
            27 => "Escape".into(),
            32 => "Space".into(),
            37 => "Left".into(),
            38 => "Up".into(),
            39 => "Right".into(),
            40 => "Down".into(),
            46 => "Delete".into(),
            _ => format!("VK({vk})"),
        }
    }

    unsafe extern "system" fn keyboard_proc(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let is_action = code as u32 == HC_ACTION;
        if is_action && lparam.0 != 0 {
            let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            // Injected events come from our own SendInput; ignore them so they
            // cannot be forwarded back to the peer (loop prevention).
            let injected = (info.dwExtraInfo & 0x1) != 0 || (info.flags.0 & 0x10) != 0;
            let message = wparam.0 as u32;
            let down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
            let up = message == WM_KEYUP || message == WM_SYSKEYUP;
            if !injected && (down || up) {
                let modifier = match info.vkCode {
                    VK_CONTROL => Some(MOD_CTRL),
                    VK_MENU => Some(MOD_ALT),
                    VK_SHIFT => Some(MOD_SHIFT),
                    _ => None,
                };
                if let Some(bit) = modifier {
                    let old = MODIFIERS.load(Ordering::SeqCst);
                    MODIFIERS.store(if down { old | bit } else { old & !bit }, Ordering::SeqCst);
                }

                if down {
                    let flags = MODIFIERS.load(Ordering::SeqCst);
                    let event = if info.vkCode == VK_M
                        && flags & (MOD_CTRL | MOD_ALT | MOD_SHIFT)
                            == (MOD_CTRL | MOD_ALT | MOD_SHIFT)
                    {
                        CapturedEvent::Hotkey(escape_hotkey())
                    } else {
                        CapturedEvent::Key {
                            key: key_name(info.vkCode),
                            ctrl: flags & MOD_CTRL != 0,
                            alt: flags & MOD_ALT != 0,
                            shift: flags & MOD_SHIFT != 0,
                        }
                    };

                    if let Some(bridge) = BRIDGE.get() {
                        let mut bridge = bridge.lock().unwrap();
                        let was_forwarding = bridge.forwarding;
                        bridge.handle(event);
                        if was_forwarding {
                            return LRESULT(1);
                        }
                    }
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let is_action = code as u32 == HC_ACTION;
        if is_action && wparam.0 as u32 == WM_MOUSEMOVE && lparam.0 != 0 {
            let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            let injected = (info.flags & 0x01) != 0 || (info.dwExtraInfo & 0x1) != 0;
            if !injected {
                let event = CapturedEvent::MouseMove {
                    x: info.pt.x,
                    y: info.pt.y,
                };
                if let Some(bridge) = BRIDGE.get() {
                    let mut bridge = bridge.lock().unwrap();
                    let was_forwarding = bridge.forwarding;
                    bridge.handle(event);
                    if was_forwarding {
                        return LRESULT(1);
                    }
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    pub(super) fn start_hooks(bridge: Arc<Mutex<InputBridge>>) -> JoinHandle<()> {
        let _ = BRIDGE.set(bridge);
        tokio::task::spawn_blocking(|| {
            unsafe {
                let Ok(module) = GetModuleHandleW(None) else {
                    return;
                };
                let hinstance = HINSTANCE(module.0 as *mut c_void);
                let Ok(keyboard) =
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), Some(hinstance), 0)
                else {
                    return;
                };
                let Ok(mouse) =
                    SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), Some(hinstance), 0)
                else {
                    let _ = UnhookWindowsHookEx(keyboard);
                    return;
                };
                *HOOKS.lock().unwrap() = vec![keyboard.0 as isize, mouse.0 as isize];
                let _guard = HookGuard(vec![keyboard.0 as isize, mouse.0 as isize]);

                let mut message = MSG::default();
                while GetMessageW(&mut message, None, 0, 0).0 > 0 {
                    let _ = TranslateMessage(&message);
                    let _ = windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&message);
                }
            }
        })
    }

    pub(super) fn inject(event: &CapturedEvent) {
        let mut inputs: Vec<INPUT> = Vec::new();
        match event {
            CapturedEvent::Key { .. } | CapturedEvent::Hotkey(_) => {
                let name = match event {
                    CapturedEvent::Key { key, .. } => key.as_str(),
                    CapturedEvent::Hotkey(name) => {
                        if *name != escape_hotkey() {
                            return;
                        }
                        "M"
                    }
                    CapturedEvent::MouseMove { .. } => unreachable!(),
                };
                // The escape hotkey releases the modifier keys to restore host
                // focus; a normal key is pressed and released.
                let base = name.rsplit('+').next().unwrap_or(name);
                let vk = base
                    .chars()
                    .next()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .map(|c| c.to_ascii_uppercase() as u16)
                    .unwrap_or(0);
                let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) as u16 };
                for flags in [KEYBD_EVENT_FLAGS::default(), KEYEVENTF_KEYUP] {
                    let mut input = INPUT::default();
                    input.r#type = INPUT_KEYBOARD;
                    input.Anonymous.ki = KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk),
                        wScan: scan,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    };
                    inputs.push(input);
                }
                if matches!(event, CapturedEvent::Hotkey(_)) {
                    for modifier in [VK_CONTROL, VK_MENU, VK_SHIFT] {
                        let mut input = INPUT::default();
                        input.r#type = INPUT_KEYBOARD;
                        input.Anonymous.ki = KEYBDINPUT {
                            wVk: VIRTUAL_KEY(modifier as u16),
                            wScan: 0,
                            dwFlags: KEYEVENTF_KEYUP,
                            time: 0,
                            dwExtraInfo: 0,
                        };
                        inputs.push(input);
                    }
                }
            }
            CapturedEvent::MouseMove { x, y } => {
                // MOUSEEVENTF_ABSOLUTE wants 0..65535 over the virtual screen.
                let origin_x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
                let origin_y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
                let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) }.max(1);
                let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) }.max(1);
                let normalize = |value: i32, origin: i32, span: i32| -> i32 {
                    let raw = (value - origin) as f64 * 65535.0 / span.max(1) as f64;
                    raw.round().clamp(0.0, 65535.0) as i32
                };
                let mut input = INPUT::default();
                input.r#type = INPUT_MOUSE;
                input.Anonymous.mi = MOUSEINPUT {
                    dx: normalize(*x, origin_x, width),
                    dy: normalize(*y, origin_y, height),
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                    time: 0,
                    dwExtraInfo: 0,
                };
                inputs.push(input);
            }
        }
        if !inputs.is_empty() {
            unsafe {
                let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            }
        }
    }

    pub(super) fn stop_hooks() {
        for hook in HOOKS.lock().unwrap().drain(..) {
            unsafe {
                let _ = UnhookWindowsHookEx(HHOOK(hook as *mut c_void));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_ctrl_c_to_command_c_on_mac_target() {
        let event = CapturedEvent::Key {
            key: "C".into(),
            ctrl: true,
            alt: false,
            shift: false,
        };
        assert_eq!(translate(&event, "mac").unwrap().key, "Cmd+C");
    }

    #[test]
    fn escape_hotkey_stops_forwarding() {
        let mut bridge = InputBridge::new();
        bridge.forwarding = true;
        bridge.handle(CapturedEvent::Hotkey("Ctrl+Alt+Shift+M"));
        assert!(!bridge.forwarding);
    }

    #[test]
    fn forwards_captured_event_as_input_event() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut bridge = InputBridge::with_sender("pc-b", tx);
        bridge.forwarding = true;
        let event = CapturedEvent::MouseMove { x: 100, y: 200 };
        bridge.handle(event.clone());
        let message = rx.try_recv().unwrap();
        match message {
            Message::InputEvent { target, payload } => {
                assert_eq!(target, "pc-b");
                assert_eq!(bincode::deserialize::<CapturedEvent>(&payload).unwrap(), event);
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }

    #[test]
    fn captured_event_survives_wire_round_trip() {
        for event in [
            CapturedEvent::Key {
                key: "C".into(),
                ctrl: true,
                alt: false,
                shift: false,
            },
            CapturedEvent::Hotkey("Ctrl+Alt+Shift+M"),
            CapturedEvent::MouseMove { x: -3, y: 7 },
        ] {
            let bytes = bincode::serialize(&event).unwrap();
            assert_eq!(bincode::deserialize::<CapturedEvent>(&bytes).unwrap(), event);
        }
        let bad = bincode::serialize(&"Not a hotkey").unwrap();
        assert!(bincode::deserialize::<CapturedEvent>(&bad).is_err());
    }
}

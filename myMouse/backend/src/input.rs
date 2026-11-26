use crate::layout::escape_hotkey;
use mymouse_protocol::Message;
use serde::de::Error as DeserializeError;
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::{Arc, Mutex, OnceLock};
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
            // Hotkeys are local control signals only. A peer must never be
            // able to synthesise our escape sequence and release host
            // modifiers or type a key.
            WireEvent::Hotkey(_) => Err(D::Error::custom("hotkeys cannot be received from a peer")),
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

/// Normalise an absolute screen coordinate to Win32's 0..65535 virtual desk
/// range. Integer math in i128 avoids overflow with hostile `i64` values.
pub fn normalize_coordinate(value: i64, origin: i32, span: i32) -> i32 {
    let width = (span.max(1) as i128).max(1);
    let scaled = (value as i128 - origin as i128) * 65_535 / width;
    scaled.clamp(0, 65_535) as i32
}

/// Optional hook used by the production runtime so the local escape hotkey
/// closes the remote-injection gate as well as stopping local capture.
static STOP_CALLBACK: OnceLock<Arc<dyn Fn() + Send + Sync>> = OnceLock::new();

pub fn set_stop_callback(callback: Arc<dyn Fn() + Send + Sync>) {
    let _ = STOP_CALLBACK.set(callback);
}

pub struct InputBridge {
    pub forwarding: bool,
    pub target: String,
    outgoing: Option<mpsc::UnboundedSender<Message>>,
    /// Production gate: local input is only consumed when a peer is actually
    /// connected. Test mode leaves this unset and keeps its existing behavior.
    connection_gate: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
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
            connection_gate: None,
        }
    }

    /// Test/transport-friendly constructor: captured events are emitted as
    /// `Message::InputEvent` on `outgoing` while forwarding is active.
    pub fn with_sender(target: &str, outgoing: mpsc::UnboundedSender<Message>) -> Self {
        Self::with_connection_gate(target, outgoing, Arc::new(|| true))
    }

    /// Production constructor. `gate` must report whether a peer is connected;
    /// without one the OS hooks would swallow local input even with no peer.
    pub fn with_connection_gate(
        target: &str,
        outgoing: mpsc::UnboundedSender<Message>,
        gate: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Self {
        Self {
            forwarding: false,
            target: target.to_string(),
            outgoing: Some(outgoing),
            connection_gate: Some(gate),
        }
    }

    /// True only while forwarding is enabled and there is a peer to receive it.
    pub fn active(&self) -> bool {
        self.forwarding
            && self
                .connection_gate
                .as_ref()
                .map(|gate| gate())
                .unwrap_or(true)
    }

    /// Handle a locally captured event. Returns whether the OS hook should
    /// suppress local delivery. When forwarding is not active the OS keeps the
    /// event; remote events are applied through `inject`, preventing loops.
    pub fn handle(&mut self, event: CapturedEvent) -> bool {
        if let CapturedEvent::Hotkey(name) = &event {
            if *name == escape_hotkey() {
                self.forwarding = false;
                // Release stuck modifiers without typing the hotkey itself,
                // and close the remote injection gate if production wired it.
                Self::release_modifiers();
                if let Some(callback) = STOP_CALLBACK.get() {
                    callback();
                }
                return true;
            }
        }

        if !self.active() {
            return false;
        }

        let Some(outgoing) = &self.outgoing else {
            return false;
        };
        let payload = bincode::serialize(&event).unwrap_or_default();
        let _ = outgoing.send(Message::InputEvent {
            target: self.target.clone(),
            payload,
        });
        true
    }

    /// Apply an event received from a peer on the local machine. Local escape
    /// hotkeys are never injectable.
    pub fn inject(event: &CapturedEvent) {
        if matches!(event, CapturedEvent::Hotkey(_)) {
            return;
        }
        #[cfg(windows)]
        win::inject(event);
        #[cfg(not(windows))]
        let _ = event;
    }

    /// Emergency release of host modifier keys after stopping forwarding.
    pub fn release_modifiers() {
        #[cfg(windows)]
        win::release_modifiers();
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
        MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT, VIRTUAL_KEY, INPUT,
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
                        if bridge.handle(event) {
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
                    if bridge.handle(event) {
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
            CapturedEvent::Key { key, .. } => {
                // A normal key is pressed and released. Hotkeys are local-only.
                let base = key.rsplit('+').next().unwrap_or(key);
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
            }
            CapturedEvent::Hotkey(_) => return,
            CapturedEvent::MouseMove { x, y } => {
                // MOUSEEVENTF_ABSOLUTE wants 0..65535 over the virtual screen.
                let origin_x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
                let origin_y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
                let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) }.max(1);
                let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) }.max(1);
                let mut input = INPUT::default();
                input.r#type = INPUT_MOUSE;
                input.Anonymous.mi = MOUSEINPUT {
                    dx: super::normalize_coordinate(i64::from(*x), origin_x, width),
                    dy: super::normalize_coordinate(i64::from(*y), origin_y, height),
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
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

    /// Release modifier keys without synthesising any character. Used when
    /// the local escape hotkey stops forwarding.
    pub(super) fn release_modifiers() {
        let mut inputs = Vec::with_capacity(3);
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
        unsafe {
            let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
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
    fn remote_hotkey_is_rejected_on_deserialize() {
        // A peer must never be able to send the local escape hotkey and make
        // us release modifiers or type a key on the host.
        let bytes = bincode::serialize(&CapturedEvent::Hotkey(escape_hotkey())).unwrap();
        assert!(bincode::deserialize::<CapturedEvent>(&bytes).is_err());
    }

    #[test]
    fn normalize_coordinate_clamps_extreme_values() {
        assert_eq!(normalize_coordinate(i64::MIN, -100, 1920), 0);
        assert_eq!(normalize_coordinate(i64::MAX, -100, 1920), 65_535);
        assert_eq!(normalize_coordinate(960, 0, 1920), 32_767);
        assert_eq!(normalize_coordinate(10, 100, 1920), 0);
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
    fn forwarding_without_a_connected_peer_keeps_local_input() {
        // Regression: production enabled forwarding at startup even with no
        // peer, so the OS hooks swallowed every local key press and mouse move.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut bridge = InputBridge::with_connection_gate("pc-b", tx, Arc::new(|| false));
        bridge.forwarding = true;
        let suppress = bridge.handle(CapturedEvent::MouseMove { x: 10, y: 20 });
        assert!(!suppress, "local input must reach the OS when no peer is connected");
        assert!(rx.try_recv().is_err(), "event must not be sent without a peer");
    }

    #[test]
    fn forwarding_with_a_connected_peer_suppresses_local_input() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut bridge = InputBridge::with_connection_gate("pc-b", tx, Arc::new(|| true));
        bridge.forwarding = true;
        let suppress = bridge.handle(CapturedEvent::MouseMove { x: 10, y: 20 });
        assert!(suppress, "input must be consumed locally when it is sent to a peer");
        assert!(rx.try_recv().is_ok());
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
            CapturedEvent::MouseMove { x: -3, y: 7 },
        ] {
            let bytes = bincode::serialize(&event).unwrap();
            assert_eq!(bincode::deserialize::<CapturedEvent>(&bytes).unwrap(), event);
        }
        let bad = bincode::serialize(&"Not a hotkey").unwrap();
        assert!(bincode::deserialize::<CapturedEvent>(&bad).is_err());
    }
}

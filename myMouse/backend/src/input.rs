use crate::layout::{escape_hotkey, Layout};
use mymouse_protocol::Message;
use serde::de::Error as DeserializeError;
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// An input event observed on the host or received from a peer.
///
/// `Hotkey` keeps a `&'static str` in the public API; deserialization maps the
/// known wire strings back to static values so no untrusted string is leaked
/// into a long-lived allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum CapturedEvent {
    Key {
        key: String,
        ctrl: bool,
        alt: bool,
        shift: bool,
        down: bool,
    },
    Hotkey(&'static str),
    MouseMove { x: i32, y: i32 },
    MouseButton {
        button: MouseButton,
        down: bool,
        x: i32,
        y: i32,
    },
    MouseWheel {
        /// Signed notches; positive scrolls up, negative scrolls down.
        delta: i32,
        x: i32,
        y: i32,
    },
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
                down: bool,
            },
            Hotkey(String),
            MouseMove { x: i32, y: i32 },
            MouseButton {
                button: MouseButton,
                down: bool,
                x: i32,
                y: i32,
            },
            MouseWheel { delta: i32, x: i32, y: i32 },
        }

        match WireEvent::deserialize(deserializer)? {
            WireEvent::Key {
                key,
                ctrl,
                alt,
                shift,
                down,
            } => Ok(CapturedEvent::Key {
                key,
                ctrl,
                alt,
                shift,
                down,
            }),
            // Hotkeys are local control signals only. A peer must never be
            // able to synthesise our escape sequence and release host
            // modifiers or type a key.
            WireEvent::Hotkey(_) => Err(D::Error::custom("hotkeys cannot be received from a peer")),
            WireEvent::MouseMove { x, y } => Ok(CapturedEvent::MouseMove { x, y }),
            WireEvent::MouseButton {
                button,
                down,
                x,
                y,
            } => Ok(CapturedEvent::MouseButton {
                button,
                down,
                x,
                y,
            }),
            WireEvent::MouseWheel { delta, x, y } => {
                Ok(CapturedEvent::MouseWheel { delta, x, y })
            }
        }
    }
}

/// Map a stable key name to a Win32 virtual-key code. Names are produced by
/// the capture hook and are intentionally independent of the source keyboard
/// layout; the receiver maps them to its own scancodes.
fn virtual_key(name: &str) -> u16 {
    const TABLE: &[(&str, u16)] = &[
        ("Backspace", 0x08),
        ("Tab", 0x09),
        ("Enter", 0x0D),
        ("Shift", 0x10),
        ("Ctrl", 0x11),
        ("Alt", 0x12),
        ("Escape", 0x1B),
        ("Space", 0x20),
        ("PageUp", 0x21),
        ("PageDown", 0x22),
        ("End", 0x23),
        ("Home", 0x24),
        ("Left", 0x25),
        ("Up", 0x26),
        ("Right", 0x27),
        ("Down", 0x28),
        ("Insert", 0x2D),
        ("Delete", 0x2E),
    ];
    if let Some((_, vk)) = TABLE.iter().find(|(key, _)| *key == name) {
        return *vk;
    }
    if let Some(func) = name.strip_prefix('F') {
        if let Ok(n) = func.parse::<u16>() {
            if (1..=24).contains(&n) {
                return 0x6F + n;
            }
        }
    }
    // Capture emits `VK(n)` when a key has no stable printable name. Parse the
    // whole number instead of falling through to the leading `V` (VK 0x56),
    // and reject out-of-range values from an untrusted peer.
    if let Some(raw) = name
        .strip_prefix("VK(")
        .and_then(|suffix| suffix.strip_suffix(')'))
    {
        if let Ok(vk) = raw.parse::<u16>() {
            if (1..=255).contains(&vk) {
                return vk;
            }
        }
        return 0;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() => c.to_ascii_uppercase() as u16,
        _ => 0,
    }
}

const MOUSEEVENTF_LEFTDOWN: u32 = 0x0002;
const MOUSEEVENTF_LEFTUP: u32 = 0x0004;
const MOUSEEVENTF_RIGHTDOWN: u32 = 0x0008;
const MOUSEEVENTF_RIGHTUP: u32 = 0x0010;
const MOUSEEVENTF_MIDDLEDOWN: u32 = 0x0020;
const MOUSEEVENTF_MIDDLEUP: u32 = 0x0040;
const MOUSEEVENTF_XDOWN: u32 = 0x0080;
const MOUSEEVENTF_XUP: u32 = 0x0100;
const MOUSEEVENTF_WHEEL: u32 = 0x0800;
const MOUSEEVENTF_HWHEEL: u32 = 0x1000;

fn mouse_button_flags(button: MouseButton, down: bool) -> u32 {
    match (button, down) {
        (MouseButton::Left, true) => MOUSEEVENTF_LEFTDOWN,
        (MouseButton::Left, false) => MOUSEEVENTF_LEFTUP,
        (MouseButton::Right, true) => MOUSEEVENTF_RIGHTDOWN,
        (MouseButton::Right, false) => MOUSEEVENTF_RIGHTUP,
        (MouseButton::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
        (MouseButton::Middle, false) => MOUSEEVENTF_MIDDLEUP,
        (MouseButton::X1, true) | (MouseButton::X2, true) => MOUSEEVENTF_XDOWN,
        (MouseButton::X1, false) | (MouseButton::X2, false) => MOUSEEVENTF_XUP,
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
        ..
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

type TargetProvider = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// Live handoff state for the topology-aware production bridge. `None` means
/// ordinary local input stays on this machine; only a configured edge crossing
/// can populate `handoff_target`.
struct TopologyControl {
    layout: Arc<RwLock<Layout>>,
    local_device_id: String,
    monitor_index: u32,
    handoff_target: Option<String>,
}

pub struct InputBridge {
    pub forwarding: bool,
    target_provider: TargetProvider,
    outgoing: Option<mpsc::UnboundedSender<Message>>,
    /// Production gate: local input is only consumed when a peer is actually
    /// connected. Test mode leaves this unset and keeps its existing behavior.
    connection_gate: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    topology: Option<TopologyControl>,
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
            target_provider: Arc::new(|| None),
            outgoing: None,
            connection_gate: None,
            topology: None,
        }
    }

    /// Test/transport-friendly constructor: captured events are emitted as
    /// `Message::InputEvent` on `outgoing` while forwarding is active.
    pub fn with_sender(target: &str, outgoing: mpsc::UnboundedSender<Message>) -> Self {
        Self::with_connection_gate(target, outgoing, Arc::new(|| true))
    }

    /// Constructor for a fixed target, used by tests and the deterministic
    /// two-instance harness. Production must use `with_target_provider`.
    pub fn with_connection_gate(
        target: &str,
        outgoing: mpsc::UnboundedSender<Message>,
        gate: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Self {
        let fixed_target = target.to_string();
        Self::with_target_provider(
            outgoing,
            gate,
            Arc::new(move || Some(fixed_target.clone())),
        )
    }

    /// Production constructor. `gate` reports whether a peer is connected and
    /// `target_provider` reports the currently connected peer's device ID.
    /// Captured events must never be stamped with the local device ID: the
    /// receiver rejects frames whose target is not itself.
    pub fn with_target_provider(
        outgoing: mpsc::UnboundedSender<Message>,
        gate: Arc<dyn Fn() -> bool + Send + Sync>,
        target_provider: TargetProvider,
    ) -> Self {
        Self {
            forwarding: false,
            target_provider,
            outgoing: Some(outgoing),
            connection_gate: Some(gate),
            topology: None,
        }
    }

    /// Production constructor. Ordinary input stays local until a `MouseMove`
    /// crosses a configured edge of the local monitor. The crossing event is
    /// sent once with target coordinates scaled by `Layout`, then subsequent
    /// events are forwarded to that same peer until Escape/local stop.
    pub fn with_topology(
        layout: Arc<RwLock<Layout>>,
        local_device_id: &str,
        monitor_index: u32,
        outgoing: mpsc::UnboundedSender<Message>,
        gate: Arc<dyn Fn() -> bool + Send + Sync>,
        target_provider: TargetProvider,
    ) -> Self {
        Self {
            forwarding: true,
            target_provider,
            outgoing: Some(outgoing),
            connection_gate: Some(gate),
            topology: Some(TopologyControl {
                layout,
                local_device_id: local_device_id.to_string(),
                monitor_index,
                handoff_target: None,
            }),
        }
    }

    /// True only while forwarding is enabled and there is a peer to receive it.
    /// In topology mode this additionally requires an active handoff to that
    /// exact peer, so connecting alone never swallows local input.
    pub fn active(&self) -> bool {
        if !self.forwarding
            || !self
                .connection_gate
                .as_ref()
                .map(|gate| gate())
                .unwrap_or(true)
        {
            return false;
        }
        match &self.topology {
            Some(topology) => {
                let Some(target) = (self.target_provider)() else {
                    return false;
                };
                topology.handoff_target.as_deref() == Some(target.as_str())
            }
            None => true,
        }
    }

    /// Handle a locally captured event. Returns whether the OS hook should
    /// suppress local delivery. When forwarding is not active the OS keeps the
    /// event; remote events are applied through `inject`, preventing loops.
    pub fn handle(&mut self, event: CapturedEvent) -> bool {
        if let CapturedEvent::Hotkey(name) = &event {
            if *name == escape_hotkey() {
                self.forwarding = false;
                if let Some(topology) = &mut self.topology {
                    topology.handoff_target = None;
                }
                // Release stuck modifiers without typing the hotkey itself,
                // and close the remote injection gate if production wired it.
                Self::release_modifiers();
                if let Some(callback) = STOP_CALLBACK.get() {
                    callback();
                }
                return true;
            }
        }

        let connected = self
            .connection_gate
            .as_ref()
            .map(|gate| gate())
            .unwrap_or(true);
        if !self.forwarding || !connected {
            if let Some(topology) = &mut self.topology {
                topology.handoff_target = None;
            }
            return false;
        }

        let current_target = (self.target_provider)();
        let mut crossing_event = None;
        if let Some(topology) = &mut self.topology {
            if let Some(handoff) = &topology.handoff_target {
                if current_target.as_deref() != Some(handoff.as_str()) {
                    topology.handoff_target = None;
                    return false;
                }
            }
            if topology.handoff_target.is_none() {
                if let CapturedEvent::MouseMove { x, y } = &event {
                    if *x >= 0 && *y >= 0 {
                        let crossing = topology.layout.read().unwrap().cross_from_index(
                            &topology.local_device_id,
                            topology.monitor_index,
                            *x as u32,
                            *y as u32,
                        );
                        if let Some(crossing) = crossing {
                            if current_target.as_deref() == Some(crossing.target.as_str()) {
                                topology.handoff_target = Some(crossing.target.clone());
                                crossing_event = Some(CapturedEvent::MouseMove {
                                    x: crossing.x as i32,
                                    y: crossing.y as i32,
                                });
                            }
                        }
                    }
                }
            }
        }
        if let Some(crossed) = crossing_event {
            return self.send_event(crossed, current_target);
        }
        if self
            .topology
            .as_ref()
            .is_some_and(|topology| topology.handoff_target.is_none())
        {
            return false;
        }
        self.send_event(event, current_target)
    }

    fn send_event(&mut self, event: CapturedEvent, target: Option<String>) -> bool {
        let sent = if let (Some(outgoing), Some(target)) = (&self.outgoing, target) {
            let payload = bincode::serialize(&event).unwrap_or_default();
            outgoing.send(Message::InputEvent { target, payload }).is_ok()
        } else {
            false
        };
        if sent {
            return true;
        }
        // A dead forwarding channel must never swallow local input. Clear the
        // handoff and pause capture so the OS passes every event through until
        // the user explicitly resumes.
        self.forwarding = false;
        if let Some(topology) = &mut self.topology {
            topology.handoff_target = None;
        }
        // A peer may be holding a modifier while the channel dies. Use the same
        // cleanup as the local escape so it cannot stay stuck after we return
        // control to this machine.
        Self::release_modifiers();
        if let Some(callback) = STOP_CALLBACK.get() {
            callback();
        }
        false
    }

    /// Emergency pause: close local capture without tearing down the OS hooks.
    /// The hooks stay installed but pass every event through until `resume`.
    pub fn pause(&mut self) {
        self.forwarding = false;
        if let Some(topology) = &mut self.topology {
            topology.handoff_target = None;
        }
    }

    /// Re-enable local capture after `pause` without restarting the process.
    pub fn resume(&mut self) {
        self.forwarding = true;
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
        MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSE_EVENT_FLAGS, MOUSEINPUT, VIRTUAL_KEY,
        INPUT,
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
    /// Modifier state already injected on the target machine. Only transitions
    /// are sent so held modifiers survive across separate key events.
    static TARGET_MODIFIERS: AtomicU8 = AtomicU8::new(0);

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
        // Named keys must have the same stable names `virtual_key` decodes;
        // otherwise an F-key or modifier is injected as a completely
        // different key on the target.
        match vk {
            8 => return "Backspace".into(),
            9 => return "Tab".into(),
            13 => return "Enter".into(),
            16 => return "Shift".into(),
            17 => return "Ctrl".into(),
            18 => return "Alt".into(),
            27 => return "Escape".into(),
            32 => return "Space".into(),
            33 => return "PageUp".into(),
            34 => return "PageDown".into(),
            35 => return "End".into(),
            36 => return "Home".into(),
            37 => return "Left".into(),
            38 => return "Up".into(),
            39 => return "Right".into(),
            40 => return "Down".into(),
            45 => return "Insert".into(),
            46 => return "Delete".into(),
            112..=135 => return format!("F{}", vk - 111),
            _ => {}
        }
        let mapped = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_CHAR) };
        let ch = char::from_u32(mapped & 0xffff)
            .filter(|c| !c.is_control())
            .unwrap_or('?');
        if ch.is_alphanumeric() {
            return ch.to_ascii_uppercase().to_string();
        }
        format!("VK({vk})")
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

                let flags = MODIFIERS.load(Ordering::SeqCst);
                // The escape hotkey is a local control signal: fire once on
                // press, never on release, and never send it to the peer.
                let event = if down
                    && info.vkCode == VK_M
                    && flags & (MOD_CTRL | MOD_ALT | MOD_SHIFT)
                        == (MOD_CTRL | MOD_ALT | MOD_SHIFT)
                {
                    Some(CapturedEvent::Hotkey(escape_hotkey()))
                } else {
                    Some(CapturedEvent::Key {
                        key: key_name(info.vkCode),
                        ctrl: flags & MOD_CTRL != 0,
                        alt: flags & MOD_ALT != 0,
                        shift: flags & MOD_SHIFT != 0,
                        down,
                    })
                };

                if let Some(event) = event {
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
        const WM_LBUTTONDOWN: u32 = 0x0201;
        const WM_LBUTTONUP: u32 = 0x0202;
        const WM_RBUTTONDOWN: u32 = 0x0204;
        const WM_RBUTTONUP: u32 = 0x0205;
        const WM_MBUTTONDOWN: u32 = 0x0207;
        const WM_MBUTTONUP: u32 = 0x0208;
        const WM_MOUSEWHEEL: u32 = 0x020A;
        const WM_XBUTTONDOWN: u32 = 0x020B;
        const WM_XBUTTONUP: u32 = 0x020C;
        const WM_MOUSEHWHEEL: u32 = 0x020E;

        let is_action = code as u32 == HC_ACTION;
        if is_action && lparam.0 != 0 {
            let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            let injected = (info.flags & 0x01) != 0 || (info.dwExtraInfo & 0x1) != 0;
            if !injected {
                let message = wparam.0 as u32;
                let event = match message {
                    WM_MOUSEMOVE => Some(CapturedEvent::MouseMove {
                        x: info.pt.x,
                        y: info.pt.y,
                    }),
                    WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP
                    | WM_MBUTTONDOWN | WM_MBUTTONUP | WM_XBUTTONDOWN | WM_XBUTTONUP => {
                        let (button, down) = match message {
                            WM_LBUTTONDOWN => (MouseButton::Left, true),
                            WM_LBUTTONUP => (MouseButton::Left, false),
                            WM_RBUTTONDOWN => (MouseButton::Right, true),
                            WM_RBUTTONUP => (MouseButton::Right, false),
                            WM_MBUTTONDOWN => (MouseButton::Middle, true),
                            WM_MBUTTONUP => (MouseButton::Middle, false),
                            WM_XBUTTONDOWN | WM_XBUTTONUP => {
                                let extra = (info.mouseData >> 16) as u16;
                                let button = if extra == 1 {
                                    MouseButton::X1
                                } else {
                                    MouseButton::X2
                                };
                                (button, message == WM_XBUTTONDOWN)
                            }
                            _ => unreachable!(),
                        };
                        Some(CapturedEvent::MouseButton {
                            button,
                            down,
                            x: info.pt.x,
                            y: info.pt.y,
                        })
                    }
                    WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                        // Mouse wheel delta lives in the high word of mouseData.
                        // Horizontal wheels are intentionally normalised to a
                        // vertical event because the wire event has one axis.
                        let delta = (info.mouseData >> 16) as i16 as i32;
                        Some(CapturedEvent::MouseWheel {
                            delta,
                            x: info.pt.x,
                            y: info.pt.y,
                        })
                    }
                    _ => None,
                };
                if let Some(event) = event {
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

    fn absolute_move(x: i32, y: i32) -> INPUT {
        let origin_x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
        let origin_y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
        let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) }.max(1);
        let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) }.max(1);
        let mut input = INPUT::default();
        input.r#type = INPUT_MOUSE;
        input.Anonymous.mi = MOUSEINPUT {
            dx: super::normalize_coordinate(i64::from(x), origin_x, width),
            dy: super::normalize_coordinate(i64::from(y), origin_y, height),
            mouseData: 0,
            dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
            time: 0,
            dwExtraInfo: 0,
        };
        input
    }

    fn key_input(vk: u16, down: bool) -> INPUT {
        let scan = unsafe { MapVirtualKeyW(u32::from(vk), MAPVK_VK_TO_VSC) as u16 };
        let mut input = INPUT::default();
        input.r#type = INPUT_KEYBOARD;
        input.Anonymous.ki = KEYBDINPUT {
            wVk: VIRTUAL_KEY(vk),
            wScan: scan,
            dwFlags: if down {
                KEYBD_EVENT_FLAGS::default()
            } else {
                KEYEVENTF_KEYUP
            },
            time: 0,
            dwExtraInfo: 0,
        };
        input
    }

    pub(super) fn inject(event: &CapturedEvent) {
        let mut inputs: Vec<INPUT> = Vec::new();
        match event {
            CapturedEvent::Key {
                key,
                ctrl,
                alt,
                shift,
                down,
            } => {
                // Synchronise target modifiers before delivering the key so a
                // sequence such as Ctrl-down, C-down, C-up, Ctrl-up works even
                // though each event crosses the wire independently.
                let desired = (*ctrl as u8) * 1 + (*alt as u8) * 2 + (*shift as u8) * 4;
                let current = TARGET_MODIFIERS.swap(desired, Ordering::SeqCst);
                for (bit, vk) in [(1u8, VK_CONTROL), (2, VK_MENU), (4, VK_SHIFT)] {
                    let was = current & bit != 0;
                    let now = desired & bit != 0;
                    if was != now {
                        inputs.push(key_input(vk as u16, now));
                    }
                }
                // Modifier keys themselves are already handled above; sending
                // them again would release them immediately.
                let vk = super::virtual_key(key);
                if vk != 0 && !matches!(key.as_str(), "Ctrl" | "Alt" | "Shift") {
                    inputs.push(key_input(vk, *down));
                }
            }
            CapturedEvent::Hotkey(_) => return,
            CapturedEvent::MouseMove { x, y } => {
                inputs.push(absolute_move(*x, *y));
            }
            CapturedEvent::MouseButton { button, down, x, y } => {
                inputs.push(absolute_move(*x, *y));
                let mut input = INPUT::default();
                input.r#type = INPUT_MOUSE;
                let data = match button {
                    MouseButton::X1 => 1,
                    MouseButton::X2 => 2,
                    _ => 0,
                };
                input.Anonymous.mi = MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: data,
                    dwFlags: MOUSE_EVENT_FLAGS(super::mouse_button_flags(*button, *down)),
                    time: 0,
                    dwExtraInfo: 0,
                };
                inputs.push(input);
            }
            CapturedEvent::MouseWheel { delta, x, y } => {
                inputs.push(absolute_move(*x, *y));
                let mut input = INPUT::default();
                input.r#type = INPUT_MOUSE;
                input.Anonymous.mi = MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: *delta as u32,
                    dwFlags: MOUSE_EVENT_FLAGS(super::MOUSEEVENTF_WHEEL),
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

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn capture_names_special_keys_for_stable_injection() {
            // Regression: unknown keys became VK(n), so F-keys, Home/End and
            // modifier keys could not be reliably recreated on the target.
            assert_eq!(key_name(0x10), "Shift");
            assert_eq!(key_name(0x11), "Ctrl");
            assert_eq!(key_name(0x12), "Alt");
            assert_eq!(key_name(0x70), "F1");
            assert_eq!(key_name(0x7B), "F12");
            assert_eq!(key_name(0x87), "F24");
            assert_eq!(key_name(0x21), "PageUp");
            assert_eq!(key_name(0x22), "PageDown");
            assert_eq!(key_name(0x23), "End");
            assert_eq!(key_name(0x24), "Home");
            assert_eq!(key_name(0x2D), "Insert");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Edge, Layout, Node, Side};
    use std::sync::RwLock;

    fn topology_layout() -> Arc<RwLock<Layout>> {
        let mut layout = Layout::new();
        layout.add_node(Node::monitor("pc-a", 0, 101, 201));
        layout.add_node(Node::monitor("pc-b", 0, 202, 101));
        layout.set_edge(Edge {
            from: "pc-a".into(),
            to: "pc-b".into(),
            side: Side::Right,
        });
        Arc::new(RwLock::new(layout))
    }

    fn assert_input_event(message: Message, expected_target: &str) -> CapturedEvent {
        match message {
            Message::InputEvent { target, payload } => {
                assert_eq!(target, expected_target);
                bincode::deserialize::<CapturedEvent>(&payload).unwrap()
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }

    #[test]
    fn translates_ctrl_c_to_command_c_on_mac_target() {
        let event = CapturedEvent::Key {
            key: "C".into(),
            ctrl: true,
            alt: false,
            shift: false,
            down: true,
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
                down: true,
            },
            CapturedEvent::MouseMove { x: -3, y: 7 },
        ] {
            let bytes = bincode::serialize(&event).unwrap();
            assert_eq!(bincode::deserialize::<CapturedEvent>(&bytes).unwrap(), event);
        }
        let bad = bincode::serialize(&"Not a hotkey").unwrap();
        assert!(bincode::deserialize::<CapturedEvent>(&bad).is_err());
    }

    #[test]
    fn key_release_survives_wire_round_trip() {
        // Regression: only keydown was forwarded, so a peer could leave keys
        // logically held after focus changes or a disconnect.
        let event = CapturedEvent::Key {
            key: "Shift".into(),
            ctrl: false,
            alt: false,
            shift: true,
            down: false,
        };
        let bytes = bincode::serialize(&event).unwrap();
        assert_eq!(bincode::deserialize::<CapturedEvent>(&bytes).unwrap(), event);
    }

    #[test]
    fn mouse_button_and_wheel_survive_wire_round_trip() {
        // Regression: the event model had no mouse buttons or wheel, so clicks
        // and scrolling could never cross a machine boundary.
        for event in [
            CapturedEvent::MouseButton {
                button: MouseButton::Left,
                down: true,
                x: 12,
                y: 34,
            },
            CapturedEvent::MouseWheel {
                delta: -120,
                x: 56,
                y: 78,
            },
        ] {
            let bytes = bincode::serialize(&event).unwrap();
            assert_eq!(bincode::deserialize::<CapturedEvent>(&bytes).unwrap(), event);
        }
    }

    #[test]
    fn special_keys_map_to_windows_virtual_keys() {
        // Regression: injection took only the first ASCII letter, so Enter,
        // arrows, Backspace and function keys became VK 0 on the target.
        assert_eq!(virtual_key("Enter"), 13);
        assert_eq!(virtual_key("Escape"), 27);
        assert_eq!(virtual_key("Space"), 32);
        assert_eq!(virtual_key("Left"), 37);
        assert_eq!(virtual_key("F5"), 0x74);
        assert_eq!(virtual_key("A"), 65);
    }

    #[test]
    fn raw_virtual_key_names_map_to_numeric_codes() {
        // Regression: key_name emits VK(n) for keys with no printable name,
        // but virtual_key read only the first character and injected 'V'.
        assert_eq!(virtual_key("VK(112)"), 112);
        assert_eq!(virtual_key("VK(16)"), 16);
        assert_eq!(virtual_key("VK(186)"), 186);
        assert_eq!(virtual_key("VK(999)"), 0);
        assert_eq!(virtual_key("VK(not-a-number)"), 0);
    }

    #[test]
    fn mouse_button_flags_distinguish_press_and_release() {
        // Regression: without per-button flags, injected clicks could never
        // distinguish down from up and would leave the target button held.
        assert_ne!(
            mouse_button_flags(MouseButton::Left, true),
            mouse_button_flags(MouseButton::Left, false)
        );
        assert_ne!(
            mouse_button_flags(MouseButton::Right, true),
            mouse_button_flags(MouseButton::Middle, true)
        );
    }

    #[test]
    fn pause_then_resume_restores_local_capture_without_restarting_hooks() {
        // Regression: RPC emergency stop unhooked the OS hooks, but resume only
        // set forwarding=true, so local input stayed dead until app restart.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut bridge = InputBridge::with_sender("pc-b", tx);
        bridge.forwarding = true;

        bridge.pause();
        assert!(!bridge.forwarding);
        assert!(!bridge.handle(CapturedEvent::MouseMove { x: 1, y: 2 }));
        assert!(rx.try_recv().is_err());

        bridge.resume();
        assert!(bridge.forwarding);
        assert!(bridge.handle(CapturedEvent::MouseMove { x: 3, y: 4 }));
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn captured_events_are_stamped_with_the_currently_connected_peer() {
        // Regression: production stamped the local device ID as the target, so
        // once IDs were unique the receiver would reject every input frame.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let active_peer = Arc::new(Mutex::new(Some("pc-b".to_string())));
        let peer_for_bridge = active_peer.clone();
        let mut bridge = InputBridge::with_target_provider(
            tx,
            Arc::new(|| true),
            Arc::new(move || peer_for_bridge.lock().unwrap().clone()),
        );
        bridge.forwarding = true;

        assert!(bridge.handle(CapturedEvent::MouseMove { x: 1, y: 2 }));
        match rx.try_recv().unwrap() {
            Message::InputEvent { target, .. } => assert_eq!(target, "pc-b"),
            other => panic!("unexpected message: {other:?}"),
        }

        *active_peer.lock().unwrap() = Some("pc-c".to_string());
        assert!(bridge.handle(CapturedEvent::MouseMove { x: 3, y: 4 }));
        match rx.try_recv().unwrap() {
            Message::InputEvent { target, .. } => assert_eq!(target, "pc-c"),
            other => panic!("unexpected message: {other:?}"),
        }

        *active_peer.lock().unwrap() = None;
        assert!(!bridge.handle(CapturedEvent::MouseMove { x: 5, y: 6 }));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn topology_bridge_keeps_local_input_until_a_configured_crossing() {
        // Regression: merely connecting a peer armed production forwarding, so
        // every local click and keystroke went to the peer from the first frame.
        let active_peer = Arc::new(Mutex::new(Some("pc-b".to_string())));
        let peer_for_bridge = active_peer.clone();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut bridge = InputBridge::with_topology(
            topology_layout(),
            "pc-a",
            0,
            tx,
            Arc::new(|| true),
            Arc::new(move || peer_for_bridge.lock().unwrap().clone()),
        );

        let key = CapturedEvent::Key {
            key: "A".into(),
            ctrl: false,
            alt: false,
            shift: false,
            down: true,
        };
        assert!(!bridge.handle(key.clone()));
        assert!(!bridge.handle(CapturedEvent::MouseMove { x: 10, y: 20 }));
        assert!(rx.try_recv().is_err());

        assert!(bridge.handle(CapturedEvent::MouseMove { x: 100, y: 100 }));
        assert_eq!(
            assert_input_event(rx.try_recv().unwrap(), "pc-b"),
            CapturedEvent::MouseMove { x: 0, y: 50 }
        );

        assert!(bridge.handle(key.clone()));
        assert_eq!(assert_input_event(rx.try_recv().unwrap(), "pc-b"), key);
    }

    #[test]
    fn topology_crossing_requires_a_connected_matching_peer() {
        let active_peer = Arc::new(Mutex::new(Some("pc-c".to_string())));
        let connected = Arc::new(Mutex::new(true));
        let peer_for_bridge = active_peer.clone();
        let connected_for_bridge = connected.clone();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut bridge = InputBridge::with_topology(
            topology_layout(),
            "pc-a",
            0,
            tx,
            Arc::new(move || *connected_for_bridge.lock().unwrap()),
            Arc::new(move || peer_for_bridge.lock().unwrap().clone()),
        );

        assert!(!bridge.handle(CapturedEvent::MouseMove { x: 100, y: 100 }));
        assert!(rx.try_recv().is_err());

        *active_peer.lock().unwrap() = Some("pc-b".to_string());
        *connected.lock().unwrap() = false;
        assert!(!bridge.handle(CapturedEvent::MouseMove { x: 100, y: 100 }));
        assert!(rx.try_recv().is_err());

        *connected.lock().unwrap() = true;
        assert!(bridge.handle(CapturedEvent::MouseMove { x: 100, y: 100 }));
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn topology_handoff_stops_when_the_peer_changes_or_disconnects() {
        let active_peer = Arc::new(Mutex::new(Some("pc-b".to_string())));
        let peer_for_bridge = active_peer.clone();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut bridge = InputBridge::with_topology(
            topology_layout(),
            "pc-a",
            0,
            tx,
            Arc::new(|| true),
            Arc::new(move || peer_for_bridge.lock().unwrap().clone()),
        );
        assert!(bridge.handle(CapturedEvent::MouseMove { x: 100, y: 100 }));
        assert!(rx.try_recv().is_ok());

        *active_peer.lock().unwrap() = None;
        assert!(!bridge.handle(CapturedEvent::Key {
            key: "B".into(),
            ctrl: false,
            alt: false,
            shift: false,
            down: true,
        }));
        assert!(rx.try_recv().is_err());

        *active_peer.lock().unwrap() = Some("pc-c".to_string());
        assert!(!bridge.handle(CapturedEvent::MouseMove { x: 10, y: 20 }));
        assert!(rx.try_recv().is_err());

        // Returning the original peer must not silently restore control; a new
        // crossing is required.
        *active_peer.lock().unwrap() = Some("pc-b".to_string());
        assert!(!bridge.handle(CapturedEvent::MouseMove { x: 10, y: 20 }));
        assert!(rx.try_recv().is_err());
        assert!(bridge.handle(CapturedEvent::MouseMove { x: 100, y: 100 }));
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn escape_and_resume_require_a_new_topology_crossing() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut bridge = InputBridge::with_topology(
            topology_layout(),
            "pc-a",
            0,
            tx,
            Arc::new(|| true),
            Arc::new(|| Some("pc-b".to_string())),
        );
        assert!(bridge.handle(CapturedEvent::MouseMove { x: 100, y: 100 }));
        assert!(rx.try_recv().is_ok());

        assert!(bridge.handle(CapturedEvent::Hotkey(escape_hotkey())));
        assert!(!bridge.handle(CapturedEvent::MouseMove { x: 10, y: 20 }));
        assert!(rx.try_recv().is_err());

        bridge.resume();
        assert!(!bridge.handle(CapturedEvent::MouseMove { x: 10, y: 20 }));
        assert!(rx.try_recv().is_err());
        assert!(bridge.handle(CapturedEvent::MouseMove { x: 100, y: 100 }));
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn pause_during_handoff_returns_input_to_the_local_machine() {
        // The revoke RPC disconnects the peer and pauses the bridge. Even if
        // the peer ID is still cached for a moment, every captured event must
        // stay local until a fresh edge crossing re-arms the handoff.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut bridge = InputBridge::with_topology(
            topology_layout(),
            "pc-a",
            0,
            tx,
            Arc::new(|| true),
            Arc::new(|| Some("pc-b".to_string())),
        );
        assert!(bridge.handle(CapturedEvent::MouseMove { x: 100, y: 100 }));
        assert!(rx.try_recv().is_ok());

        bridge.pause();
        assert!(!bridge.active());
        assert!(!bridge.handle(CapturedEvent::Key {
            key: "A".into(),
            ctrl: false,
            alt: false,
            shift: false,
            down: true,
        }));
        assert!(rx.try_recv().is_err());

        bridge.resume();
        assert!(!bridge.handle(CapturedEvent::MouseMove { x: 10, y: 20 }));
        assert!(rx.try_recv().is_err());
        assert!(bridge.handle(CapturedEvent::MouseMove { x: 100, y: 100 }));
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn topology_crossing_never_suppresses_local_input_when_the_send_fails() {
        // Regression: the bridge reported success even after the forwarding
        // channel was gone, so a dead sender could swallow all local input.
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drop(rx);
        let mut bridge = InputBridge::with_topology(
            topology_layout(),
            "pc-a",
            0,
            tx,
            Arc::new(|| true),
            Arc::new(|| Some("pc-b".to_string())),
        );

        assert!(!bridge.handle(CapturedEvent::MouseMove { x: 100, y: 100 }));
        assert!(!bridge.active());
        assert!(!bridge.handle(CapturedEvent::MouseMove { x: 10, y: 20 }));
    }
}

//! 全局输入后端。
//!
//! Linux 现实：
//! - **evdev**（`/dev/input/event*`）是唯一能在 X11 与 Wayland 下都拿到"任意按键"的途径，
//!   需要 udev `uaccess` 规则或把用户加入 `input` 组；
//! - **X11**（XRecord/XInput2）在 KDE Wayland 下默认只能看到按住修饰键的按键，
//!   因此本 crate 用它只做"全局光标位置"这类 X11 仍开放的能力；
//! - Wayland 下客户端拿不到全局光标坐标，只能靠宠物窗口自身的输入区域做"局部躲避"。

pub mod evdev_source;
#[cfg(feature = "x11")]
pub mod x11;

use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Mutex;
use std::time::Instant;
use std::thread;
use std::time::Duration;
use typingpet_core::keys::{KeyModifiers, KeyStroke};

/// 一次按键事件（只关心"按下"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub stroke: KeyStroke,
    /// 来自哪个设备（便于日志/排查）
    pub device_index: usize,
}

/// 光标位置（X11 root 坐标系：原点左上、y 向下）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerPosition {
    pub x: f64,
    pub y: f64,
}

/// 屏幕尺寸（像素）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenSize {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Evdev,
    Mock,
    /// 拿不到键盘设备（缺权限 / 容器里没有 /dev/input）：程序继续跑，后台自动重试。
    Unavailable,
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Evdev => "evdev",
            Self::Mock => "mock",
            Self::Unavailable => "unavailable",
        }
    }
}

/// 不可用时多久重试一次。
const RETRY_INTERVAL: Duration = Duration::from_secs(2);

struct InputState {
    receiver: Option<Receiver<KeyEvent>>,
    backend: Backend,
    description: String,
    last_attempt: Option<Instant>,
    reason: Option<String>,
}

/// 运行中的输入源：一个后台线程 + 一个 channel。
///
/// 拿不到键盘设备时**不会失败**：退化成 `Backend::Unavailable`，
/// 桌宠照常显示（悬停/拖动/设置/托盘都能用），并由 `try_recv` 每 2 秒自动重试，
/// 用户装好 udev 规则或重新登录后无需重启程序即可恢复。
pub struct InputHandle {
    state: Mutex<InputState>,
}

impl InputHandle {
    fn from_parts(receiver: Option<Receiver<KeyEvent>>, backend: Backend, description: String, reason: Option<String>) -> Self {
        Self {
            state: Mutex::new(InputState {
                receiver,
                backend,
                description,
                last_attempt: Some(Instant::now()),
                reason,
            }),
        }
    }

    pub fn backend(&self) -> Backend {
        self.state.lock().map(|state| state.backend).unwrap_or(Backend::Unavailable)
    }

    pub fn description(&self) -> String {
        self.state
            .lock()
            .map(|state| state.description.clone())
            .unwrap_or_default()
    }

    pub fn is_connected(&self) -> bool {
        self.backend() != Backend::Unavailable
    }

    /// 不可用时的原因（用于 UI 提示）；可用时为 None。
    pub fn unavailable_reason(&self) -> Option<String> {
        self.state.lock().ok().and_then(|state| state.reason.clone())
    }

    /// 取一个按键事件。不可用时会按节流自动重连。
    pub fn try_recv(&self) -> Option<KeyEvent> {
        {
            let Ok(state) = self.state.lock() else {
                return None;
            };
            if let Some(receiver) = &state.receiver {
                return receiver.try_recv().ok();
            }
            let due = state
                .last_attempt
                .map(|at| at.elapsed() >= RETRY_INTERVAL)
                .unwrap_or(true);
            if !due {
                return None;
            }
        }

        // 到点重试：不持锁做打开动作（内部会 spawn 线程），避免阻塞调用方
        if let Ok(mut state) = self.state.lock() {
            state.last_attempt = Some(Instant::now());
        }
        match open_evdev() {
            Ok(handle) => {
                let description = handle.description();
                if let Ok(mut state) = self.state.lock() {
                    state.receiver = handle.take_receiver();
                    state.backend = Backend::Evdev;
                    state.description = description.clone();
                    state.reason = None;
                }
                println!("键输入已连接：{description}");
            }
            Err(error) => {
                if let Ok(mut state) = self.state.lock() {
                    state.reason = Some(error.to_string());
                }
            }
        }
        None
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Option<KeyEvent> {
        {
            let Ok(state) = self.state.lock() else {
                return None;
            };
            if let Some(receiver) = &state.receiver {
                return match receiver.recv_timeout(timeout) {
                    Ok(event) => Some(event),
                    Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => None,
                };
            }
        }
        // 不可用：睡一小会儿再走重试逻辑
        thread::sleep(timeout.min(Duration::from_millis(200)));
        self.try_recv()
    }

    fn take_receiver(&self) -> Option<Receiver<KeyEvent>> {
        self.state.lock().ok().and_then(|mut state| state.receiver.take())
    }
}

/// 打开 evdev 全局键盘监听；没有任何可用键盘设备时返回错误（调用方可降级）。
pub fn open_evdev() -> anyhow::Result<InputHandle> {
    let (sender, receiver) = mpsc::channel();
    let devices = evdev_source::spawn_keyboard_listeners(sender)?;
    Ok(InputHandle::from_parts(
        Some(receiver),
        Backend::Evdev,
        format!("evdev: {devices} keyboard device(s)"),
        None,
    ))
}

/// 打不开键盘设备时**不报错**，返回一个会自动重试的句柄（桌宠照常显示）。
pub fn open_evdev_or_retry() -> InputHandle {
    match open_evdev() {
        Ok(handle) => handle,
        Err(error) => InputHandle::from_parts(
            None,
            Backend::Unavailable,
            format!("不可用（{error}）"),
            Some(error.to_string()),
        ),
    }
}

/// 用于测试/演示的 mock 输入源：按脚本回放按键。
pub fn open_mock(script: Vec<KeyStroke>, interval: Duration) -> InputHandle {
    let (sender, receiver) = mpsc::channel();
    let count = script.len();
    thread::spawn(move || {
        for (index, stroke) in script.into_iter().enumerate() {
            if sender
                .send(KeyEvent {
                    stroke,
                    device_index: usize::MAX,
                })
                .is_err()
            {
                return;
            }
            thread::sleep(interval);
            let _ = index;
        }
    });
    InputHandle::from_parts(
        Some(receiver),
        Backend::Mock,
        format!("mock: {count} scripted keystroke(s)"),
        None,
    )
}

/// 判断当前是否运行在 X11 会话下（有 DISPLAY 且没有 Wayland socket）。
pub fn is_x11_session() -> bool {
    #[cfg(feature = "x11")]
    {
        x11::is_available()
    }
    #[cfg(not(feature = "x11"))]
    {
        false
    }
}

/// 当前会话类型，仅用于日志/UI 提示。
pub fn session_kind() -> &'static str {
    let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    if session_type.eq_ignore_ascii_case("wayland") {
        "wayland"
    } else if session_type.eq_ignore_ascii_case("x11") {
        "x11"
    } else if is_x11_session() {
        "x11"
    } else {
        "unknown"
    }
}

/// 全局光标位置。Wayland 下返回 `None`（协议不允许客户端查询全局光标）。
pub fn pointer_position() -> Option<PointerPosition> {
    if session_kind() == "wayland" && std::env::var_os("DISPLAY").is_none() {
        return None;
    }
    #[cfg(feature = "x11")]
    {
        x11::pointer_position()
    }
    #[cfg(not(feature = "x11"))]
    {
        None
    }
}

/// 屏幕尺寸（像素）。
pub fn screen_size() -> Option<ScreenSize> {
    #[cfg(feature = "x11")]
    {
        x11::screen_size()
    }
    #[cfg(not(feature = "x11"))]
    {
        None
    }
}

/// 修饰键位判定，供各后端共用。
pub fn modifier_bit_for(code: u32) -> Option<u8> {
    use typingpet_core::keys::KeyModifiers as M;
    match code {
        29 | 97 => Some(M::CONTROL),
        56 | 100 | 99 => Some(M::ALT),
        42 | 54 => Some(M::SHIFT),
        125 | 126 => Some(M::SUPER),
        _ => None,
    }
}

/// 更新修饰键状态（按下置位、松开清位）。
pub fn update_modifiers(modifiers: &mut KeyModifiers, code: u32, pressed: bool) {
    if let Some(bit) = modifier_bit_for(code) {
        modifiers.set(bit, pressed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_bits_match_evdev_codes() {
        assert_eq!(modifier_bit_for(29), Some(KeyModifiers::CONTROL));
        assert_eq!(modifier_bit_for(97), Some(KeyModifiers::CONTROL));
        assert_eq!(modifier_bit_for(56), Some(KeyModifiers::ALT));
        assert_eq!(modifier_bit_for(42), Some(KeyModifiers::SHIFT));
        assert_eq!(modifier_bit_for(125), Some(KeyModifiers::SUPER));
        assert_eq!(modifier_bit_for(37), None);
    }

    #[test]
    fn modifiers_track_press_and_release() {
        let mut modifiers = KeyModifiers::empty();
        update_modifiers(&mut modifiers, 29, true);
        assert!(modifiers.contains(KeyModifiers::CONTROL));
        update_modifiers(&mut modifiers, 29, false);
        assert!(!modifiers.contains(KeyModifiers::CONTROL));
    }

    #[test]
    fn mock_source_replays_script() {
        let handle = open_mock(
            vec![
                KeyStroke::new(37, KeyModifiers::NONE),
                KeyStroke::new(38, KeyModifiers::empty().with(KeyModifiers::CONTROL)),
            ],
            Duration::from_millis(1),
        );
        let first = handle.recv_timeout(Duration::from_secs(1)).expect("first");
        assert_eq!(first.stroke.code, 37);
        let second = handle.recv_timeout(Duration::from_secs(1)).expect("second");
        assert_eq!(second.stroke.code, 38);
        assert_eq!(handle.backend(), Backend::Mock);
    }
}

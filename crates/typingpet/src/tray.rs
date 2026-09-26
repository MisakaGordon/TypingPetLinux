//! 托盘图标（StatusNotifierItem）+ 完整菜单。
//!
//! 菜单动作不直接操作窗口，而是投递到 `TrayState::commands` 队列，
//! 由 GTK 主循环（`PetUi::drain_tray_commands`）取出执行 —— 这样跨线程只共享一个 `Mutex`，
//! 不需要把窗口类型变成 `Send`。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrayCommand {
    ToggleVisibility,
    ToggleAlwaysOnTop,
    ToggleClickThrough,
    ResetPosition,
    SetScale(f64),
    OpenSettings,
    Quit,
}

/// 托盘菜单需要展示的状态（由主循环同步）+ 待执行命令。
#[derive(Debug, Clone)]
pub struct TrayState {
    pub pet_visible: bool,
    pub always_on_top: bool,
    pub click_through: bool,
    pub scale: f64,
    pub input_status: String,
    pub session: String,
    pub reaction_count: usize,
    pub set_count: usize,
    pub commands: Vec<TrayCommand>,
}

impl TrayState {
    pub fn new(input_status: String, session: String) -> Self {
        Self {
            pet_visible: true,
            always_on_top: true,
            click_through: false,
            scale: 0.62,
            input_status,
            session,
            reaction_count: 0,
            set_count: 0,
            commands: Vec::new(),
        }
    }
}

pub type SharedTray = Arc<Mutex<TrayState>>;

static TRAY_HANDLE: OnceLock<ksni::blocking::Handle<PetTray>> = OnceLock::new();
/// 托盘是否成功注册；未注册时不再尝试刷新菜单。
static TRAY_ACTIVE: AtomicBool = AtomicBool::new(false);

pub struct PetTray {
    shared: SharedTray,
}

impl PetTray {
    fn push(&self, command: TrayCommand) {
        if let Ok(mut state) = self.shared.lock() {
            state.commands.push(command);
        }
    }
}

fn activate_for(command: TrayCommand) -> Box<dyn Fn(&mut PetTray) + Send + 'static> {
    Box::new(move |tray: &mut PetTray| tray.push(command))
}

impl ksni::Tray for PetTray {
    fn id(&self) -> String {
        "typingpet".to_string()
    }

    fn title(&self) -> String {
        "TypingPet".to_string()
    }

    fn icon_name(&self) -> String {
        "input-keyboard".to_string()
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{CheckmarkItem, MenuItem, StandardItem, SubMenu};

        let state = self.shared.lock().map(|state| state.clone()).unwrap_or_else(|_| {
            TrayState::new("unknown".to_string(), "unknown".to_string())
        });

        let visibility_label = if state.pet_visible {
            "隐藏宠物"
        } else {
            "显示宠物"
        };

        let size_item = |label: &str, value: f64| -> MenuItem<Self> {
            StandardItem {
                label: label.to_string(),
                activate: activate_for(TrayCommand::SetScale(value)),
                ..Default::default()
            }
            .into()
        };

        vec![
            StandardItem {
                label: "设置…".to_string(),
                icon_name: "preferences-system".to_string(),
                activate: activate_for(TrayCommand::OpenSettings),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: visibility_label.to_string(),
                activate: activate_for(TrayCommand::ToggleVisibility),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            CheckmarkItem {
                label: "始终置顶".to_string(),
                checked: state.always_on_top,
                activate: activate_for(TrayCommand::ToggleAlwaysOnTop),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "位置锁定（点击穿透）".to_string(),
                checked: state.click_through,
                activate: activate_for(TrayCommand::ToggleClickThrough),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "位置重置".to_string(),
                activate: activate_for(TrayCommand::ResetPosition),
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "大小".to_string(),
                submenu: vec![
                    size_item("小", 0.46),
                    size_item("中", 0.62),
                    size_item("大", 0.82),
                    size_item("特大", 1.08),
                ],
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: format!("键输入: {}", state.input_status),
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: format!("会话: {}", state.session),
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: format!("反应图片: {} 张 · 图片集: {}", state.reaction_count, state.set_count),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "退出 TypingPet".to_string(),
                icon_name: "application-exit".to_string(),
                activate: activate_for(TrayCommand::Quit),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// 注册托盘；失败只打印日志（例如没有 StatusNotifierItem 宿主）。
pub fn spawn(shared: SharedTray) {
    use ksni::blocking::TrayMethods;

    let tray = PetTray { shared };
    match tray.spawn() {
        Ok(handle) => {
            TRAY_ACTIVE.store(true, Ordering::Relaxed);
            let _ = TRAY_HANDLE.set(handle);
            println!("tray: 已注册 StatusNotifierItem");
        }
        Err(error) => {
            eprintln!("tray: 注册失败（当前桌面没有 SNI 宿主？）: {error}");
        }
    }
}

/// 通知托盘重建菜单（状态变化后调用）。
pub fn refresh() {
    if !TRAY_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    if let Some(handle) = TRAY_HANDLE.get() {
        let _ = handle.update(|_| {});
    }
}

/// 主循环调用：取出并清空待执行命令。
pub fn drain(shared: &SharedTray) -> Vec<TrayCommand> {
    match shared.lock() {
        Ok(mut state) => std::mem::take(&mut state.commands),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_queued_and_drained_once() {
        let shared: SharedTray = Arc::new(Mutex::new(TrayState::new(
            "mock".to_string(),
            "wayland".to_string(),
        )));
        {
            let mut state = shared.lock().unwrap();
            state.commands.push(TrayCommand::ToggleVisibility);
            state.commands.push(TrayCommand::SetScale(0.82));
        }
        let commands = drain(&shared);
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0], TrayCommand::ToggleVisibility);
        assert_eq!(commands[1], TrayCommand::SetScale(0.82));
        assert!(drain(&shared).is_empty(), "命令只能被消费一次");
    }
}

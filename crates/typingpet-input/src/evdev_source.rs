//! evdev 全局键盘监听。
//!
//! 每个可用键盘设备一个读取线程，全部投递到同一个 channel。
//! 只读取 `EV_KEY` 的按下/抬起，不读取 `EV_MSC`（扫描码）或任何字符输入，也不落盘。

use crate::{modifier_bit_for, KeyEvent};
use anyhow::{anyhow, Context, Result};
use evdev::{Device, EventType, KeyCode};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;
use typingpet_core::keys::KeyModifiers;

/// 候选键盘设备：至少支持字母键与回车。
fn looks_like_keyboard(device: &Device) -> bool {
    match device.supported_keys() {
        Some(keys) => {
            keys.contains(KeyCode::KEY_A)
                && keys.contains(KeyCode::KEY_Z)
                && keys.contains(KeyCode::KEY_ENTER)
        }
        None => false,
    }
}

/// 枚举键盘设备并为每个设备启动读取线程，返回启动的设备数量。
pub fn spawn_keyboard_listeners(sender: Sender<KeyEvent>) -> Result<usize> {
    let mut started = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for (index, (path, device)) in evdev::enumerate().enumerate() {
        if !looks_like_keyboard(&device) {
            continue;
        }
        let device_path: PathBuf = path;
        let mut device = match Device::open(&device_path) {
            Ok(device) => device,
            Err(error) => {
                errors.push(format!("{}: {error}", device_path.display()));
                continue;
            }
        };
        if let Err(error) = device.set_nonblocking(false) {
            errors.push(format!("{}: {error}", device_path.display()));
            continue;
        }

        let sender = sender.clone();
        let thread_path = device_path.clone();
        thread::Builder::new()
            .name(format!("typingpet-evdev-{index}"))
            .spawn(move || {
                let mut modifiers = KeyModifiers::empty();
                loop {
                    let events = match device.fetch_events() {
                        Ok(events) => events,
                        Err(error) => {
                            eprintln!(
                                "[typingpet] device {} read error: {error}",
                                thread_path.display()
                            );
                            return;
                        }
                    };
                    for event in events {
                        if event.event_type() != EventType::KEY {
                            continue;
                        }
                        let code = u32::from(event.code());
                        let pressed = match event.value() {
                            0 => false,
                            1 => true,
                            // 2 = 自动重复：忽略，避免连打时疯狂换图
                            _ => continue,
                        };

                        if modifier_bit_for(code).is_some() {
                            // 修饰键自身也要先更新状态，再决定是否上报
                            crate::update_modifiers(&mut modifiers, code, pressed);
                            continue;
                        }
                        if !pressed {
                            continue;
                        }

                        let key_event = KeyEvent {
                            stroke: typingpet_core::keys::KeyStroke::new(code, modifiers),
                            device_index: index,
                        };
                        if sender.send(key_event).is_err() {
                            return;
                        }
                    }
                }
            })
            .with_context(|| format!("spawn reader for {}", device_path.display()))?;
        started += 1;
    }

    if started == 0 {
        let detail = if errors.is_empty() {
            "no readable keyboard device found (missing permission for /dev/input/* or no /dev/input)"
                .to_string()
        } else {
            errors.join("; ")
        };
        return Err(anyhow!(detail));
    }
    Ok(started)
}

/// 列出可见的输入设备信息，供 `typingpet-probe` 使用。
pub fn describe_devices() -> Vec<String> {
    let mut lines = Vec::new();
    for (path, device) in evdev::enumerate() {
        let name = device.name().unwrap_or("<unnamed>").to_string();
        let keys = device
            .supported_keys()
            .map(|keys| keys.iter().count())
            .unwrap_or(0);
        let keyboard = looks_like_keyboard(&device);
        lines.push(format!(
            "{} | {} | keys={} | keyboard={}",
            path.display(),
            name,
            keys,
            if keyboard { "yes" } else { "no" }
        ));
    }
    lines
}

//! 按键模型与键反应规则存储。
//!
//! 与 macOS 版的差异：键码使用 **evdev code**（`KEY_*`，如 `KEY_K=37`），
//! 而不是 macOS 虚拟键码（`40`）。两者含义不同、配置不互通。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KeyModifiers(pub u8);

impl KeyModifiers {
    pub const CONTROL: u8 = 1 << 0;
    pub const ALT: u8 = 1 << 1;
    pub const SHIFT: u8 = 1 << 2;
    pub const SUPER: u8 = 1 << 3;

    pub const NONE: KeyModifiers = KeyModifiers(0);

    pub fn empty() -> Self {
        Self(0)
    }

    pub fn contains(self, bit: u8) -> bool {
        self.0 & bit != 0
    }

    pub fn with(self, bit: u8) -> Self {
        Self(self.0 | bit)
    }

    pub fn without(self, bit: u8) -> Self {
        Self(self.0 & !bit)
    }

    pub fn set(&mut self, bit: u8, enabled: bool) {
        if enabled {
            self.0 |= bit;
        } else {
            self.0 &= !bit;
        }
    }

    /// Linux 习惯的显示前缀：`Ctrl+Alt+Shift+Super+`
    pub fn display_prefix(self) -> String {
        let mut value = String::new();
        if self.contains(Self::CONTROL) {
            value.push_str("Ctrl+");
        }
        if self.contains(Self::ALT) {
            value.push_str("Alt+");
        }
        if self.contains(Self::SHIFT) {
            value.push_str("Shift+");
        }
        if self.contains(Self::SUPER) {
            value.push_str("Super+");
        }
        value
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyStroke {
    /// evdev 键码（`linux/input-event-codes.h` 中的 `KEY_*`）
    pub code: u32,
    pub modifiers: KeyModifiers,
}

impl KeyStroke {
    pub fn new(code: u32, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }

    pub fn display_name(&self) -> String {
        format!(
            "{}{}",
            self.modifiers.display_prefix(),
            evdev_key_name(self.code)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyReactionRule {
    pub id: String,
    pub stroke: KeyStroke,
    pub image_file_name: String,
}

/// 规则存储：JSON 文件 + 图片副本目录。
#[derive(Debug)]
pub struct KeyReactionStore {
    directory: PathBuf,
    store_path: PathBuf,
    rules: Vec<KeyReactionRule>,
}

impl KeyReactionStore {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        let directory = directory.into();
        let store_path = directory.join("rules.json");
        let rules = fs::read_to_string(&store_path)
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<KeyReactionRule>>(&text).ok())
            .unwrap_or_default();
        Self {
            directory,
            store_path,
            rules,
        }
    }

    pub fn rules(&self) -> &[KeyReactionRule] {
        &self.rules
    }

    pub fn image_path(&self, stroke: KeyStroke) -> Option<PathBuf> {
        self.rules
            .iter()
            .find(|rule| rule.stroke == stroke)
            .and_then(|rule| self.image_path_for(rule))
    }

    pub fn image_path_for(&self, rule: &KeyReactionRule) -> Option<PathBuf> {
        let path = self.directory.join(&rule.image_file_name);
        path.is_file().then_some(path)
    }

    pub fn set_rule(&mut self, stroke: KeyStroke, source: &Path) -> std::io::Result<KeyReactionRule> {
        fs::create_dir_all(&self.directory)?;
        let extension = source
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !crate::library::SUPPORTED_EXTENSIONS.contains(&extension.as_str()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unsupported image format: {}", source.display()),
            ));
        }
        let file_name = format!("{}.{}", crate::unique_id(), extension);
        fs::copy(source, self.directory.join(&file_name))?;
        self.rules.retain(|rule| rule.stroke != stroke);
        let rule = KeyReactionRule {
            id: crate::unique_id(),
            stroke,
            image_file_name: file_name,
        };
        self.rules.push(rule.clone());
        self.rules
            .sort_by(|left, right| left.stroke.display_name().cmp(&right.stroke.display_name()));
        self.persist()?;
        Ok(rule)
    }

    pub fn remove_rule(&mut self, id: &str) -> std::io::Result<()> {
        self.rules.retain(|rule| rule.id != id);
        self.persist()
    }

    fn persist(&self) -> std::io::Result<()> {
        fs::create_dir_all(&self.directory)?;
        let text = serde_json::to_string_pretty(&self.rules)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
        fs::write(&self.store_path, text)
    }
}

/// evdev 键码 → 显示名。只覆盖常用键，其余回退为 `KEY_<code>`。
pub fn evdev_key_name(code: u32) -> String {
    let name = match code {
        1 => "Esc",
        2 => "1",
        3 => "2",
        4 => "3",
        5 => "4",
        6 => "5",
        7 => "6",
        8 => "7",
        9 => "8",
        10 => "9",
        11 => "0",
        12 => "-",
        13 => "=",
        14 => "Backspace",
        15 => "Tab",
        16 => "Q",
        17 => "W",
        18 => "E",
        19 => "R",
        20 => "T",
        21 => "Y",
        22 => "U",
        23 => "I",
        24 => "O",
        25 => "P",
        26 => "[",
        27 => "]",
        28 => "Enter",
        29 => "Ctrl",
        30 => "A",
        31 => "S",
        32 => "D",
        33 => "F",
        34 => "G",
        35 => "H",
        36 => "J",
        37 => "K",
        38 => "L",
        39 => ";",
        40 => "'",
        41 => "`",
        42 => "Shift",
        43 => "\\",
        44 => "Z",
        45 => "X",
        46 => "C",
        47 => "V",
        48 => "B",
        49 => "N",
        50 => "M",
        51 => ",",
        52 => ".",
        53 => "/",
        54 => "Shift",
        55 => "KP*",
        56 => "Alt",
        57 => "Space",
        58 => "CapsLock",
        59 => "F1",
        60 => "F2",
        61 => "F3",
        62 => "F4",
        63 => "F5",
        64 => "F6",
        65 => "F7",
        66 => "F8",
        67 => "F9",
        68 => "F10",
        87 => "F11",
        88 => "F12",
        96 => "KP Enter",
        97 => "Ctrl",
        98 => "/",
        99 => "Alt",
        100 => "Alt",
        102 => "Home",
        103 => "↑",
        104 => "Page Up",
        105 => "←",
        106 => "→",
        107 => "End",
        108 => "↓",
        109 => "Page Down",
        110 => "Insert",
        111 => "Delete",
        119 => "Pause",
        125 => "Super",
        126 => "Super",
        127 => "Menu",
        other => return format!("KEY_{other}"),
    };
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_key_with_different_modifiers_is_different_stroke() {
        let plain = KeyStroke::new(37, KeyModifiers::NONE);
        let ctrl = KeyStroke::new(37, KeyModifiers::empty().with(KeyModifiers::CONTROL));
        assert_ne!(plain, ctrl);
        assert_eq!(ctrl.display_name(), "Ctrl+K");
    }

    #[test]
    fn modifier_prefix_follows_linux_order() {
        let modifiers = KeyModifiers::empty()
            .with(KeyModifiers::SHIFT)
            .with(KeyModifiers::CONTROL)
            .with(KeyModifiers::SUPER);
        assert_eq!(modifiers.display_prefix(), "Ctrl+Shift+Super+");
    }

    #[test]
    fn set_and_clear_modifier_bits() {
        let mut modifiers = KeyModifiers::empty();
        modifiers.set(KeyModifiers::ALT, true);
        assert!(modifiers.contains(KeyModifiers::ALT));
        modifiers.set(KeyModifiers::ALT, false);
        assert!(!modifiers.contains(KeyModifiers::ALT));
    }

    #[test]
    fn rule_persists_and_matches_exactly() {
        let root = std::env::temp_dir().join(format!("typingpet-keytest-{}", crate::unique_id()));
        let source_dir = root.join("src");
        fs::create_dir_all(&source_dir).expect("create source dir");
        let source = source_dir.join("pet-left.png");
        fs::write(&source, b"\x89PNG\r\n\x1a\n not-a-real-png").expect("write fake png");

        let store_dir = root.join("Rules");
        let stroke = KeyStroke::new(37, KeyModifiers::empty().with(KeyModifiers::CONTROL));
        {
            let mut store = KeyReactionStore::new(&store_dir);
            store
                .set_rule(stroke, &source)
                .expect("set rule should accept supported extension");
            assert!(store.image_path(stroke).is_some());
            assert!(store.image_path(KeyStroke::new(37, KeyModifiers::NONE)).is_none());
        }

        let restored = KeyReactionStore::new(&store_dir);
        assert_eq!(restored.rules().len(), 1);
        assert!(restored.image_path(stroke).is_some());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn unsupported_extension_is_rejected() {
        let root = std::env::temp_dir().join(format!("typingpet-keytest-{}", crate::unique_id()));
        fs::create_dir_all(&root).expect("create root");
        let source = root.join("note.txt");
        fs::write(&source, b"hello").expect("write file");
        let mut store = KeyReactionStore::new(root.join("Rules"));
        assert!(store.set_rule(KeyStroke::new(37, KeyModifiers::NONE), &source).is_err());
        fs::remove_dir_all(&root).ok();
    }
}

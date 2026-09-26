//! 配置存储：单个 JSON 文件，写入使用「临时文件 + rename」保证原子性。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub scale: f64,
    pub resting_opacity: f64,
    pub hover_opacity: f64,
    pub shake_level: u8,
    pub always_on_top: bool,
    pub position_locked: bool,
    pub avoids_pointer_when_locked: bool,
    /// 宠物窗口左上角（Wayland layer-shell margin / X11 窗口坐标，屏幕像素）
    pub pet_x: f64,
    pub pet_y: f64,
    /// 真实键盘设备不可用时的提示状态（仅记录，便于 UI 显示）
    pub input_backend: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            scale: 0.62,
            resting_opacity: 1.0,
            hover_opacity: 0.3,
            shake_level: 2,
            always_on_top: true,
            position_locked: false,
            avoids_pointer_when_locked: false,
            pet_x: -1.0,
            pet_y: -1.0,
            input_backend: String::new(),
        }
    }
}

impl Settings {
    pub fn clamped(mut self) -> Self {
        self.scale = self.scale.clamp(
            crate::geom::PetResizeGeometry::MINIMUM_SCALE,
            crate::geom::PetResizeGeometry::MAXIMUM_SCALE,
        );
        self.resting_opacity = self.resting_opacity.clamp(0.0, 1.0);
        self.hover_opacity = self.hover_opacity.clamp(0.0, 1.0);
        self.shake_level = self.shake_level.min(3);
        self
    }

    pub fn shake_amplitude(&self) -> f64 {
        const AMPLITUDES: [f64; 4] = [0.0, 4.0, 9.0, 15.0];
        AMPLITUDES[self.shake_level.min(3) as usize]
    }

    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
            .unwrap_or_default()
            .clamped()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, text)?;
        fs::rename(&temporary, path)
    }
}

/// 默认配置路径：`$XDG_CONFIG_HOME/typingpet/config.json`
pub fn default_config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("typingpet").join("config.json")
}

/// 默认数据目录：`$XDG_DATA_HOME/typingpet`
pub fn default_data_directory() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("typingpet")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_clamping() {
        let root = std::env::temp_dir().join(format!("typingpet-config-{}", crate::unique_id()));
        let path = root.join("config.json");

        let mut settings = Settings::default();
        settings.scale = 3.0;
        settings.hover_opacity = -1.0;
        settings.shake_level = 9;
        settings.pet_x = 42.0;
        let clamped = settings.clamped();
        assert_eq!(clamped.scale, 1.25);
        assert_eq!(clamped.hover_opacity, 0.0);
        assert_eq!(clamped.shake_level, 3);

        clamped.save(&path).expect("save");
        let loaded = Settings::load(&path);
        assert_eq!(loaded, clamped);
        assert_eq!(loaded.pet_x, 42.0);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_file_yields_defaults() {
        let path = std::env::temp_dir().join("typingpet-does-not-exist/config.json");
        let settings = Settings::load(&path);
        assert_eq!(settings.scale, Settings::default().scale);
    }

    #[test]
    fn shake_amplitude_matches_macos_table() {
        let mut settings = Settings::default();
        for (level, expected) in [(0_u8, 0.0), (1, 4.0), (2, 9.0), (3, 15.0)] {
            settings.shake_level = level;
            assert_eq!(settings.shake_amplitude(), expected);
        }
    }
}

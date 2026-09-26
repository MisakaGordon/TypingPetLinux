//! TypingPet 的平台无关核心逻辑。
//!
//! 本 crate 不依赖任何 GUI 或设备 API，因此可以在 CI/容器里完整跑测试。
//! 对应 macOS 版 `Sources/TypingPet/PetWindowGeometry.swift`、`ImagePicker.swift`、
//! `PetImageLibrary.swift`、`KeyReactions.swift` 中被平台层复用的部分。

pub mod config;
pub mod geom;
pub mod keys;
pub mod library;
pub mod picker;
pub mod state;

use std::sync::atomic::{AtomicU64, Ordering};

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 读取图片像素尺寸（不解码整张图）；失败返回 `None`。
pub fn image_dimensions(path: &std::path::Path) -> Option<geom::Size> {
    image::image_dimensions(path)
        .ok()
        .map(|(width, height)| geom::Size::new(f64::from(width), f64::from(height)))
}

/// 简易唯一 ID：时间戳（纳秒）+ 自增计数，避免引入 uuid 依赖。
pub fn unique_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:x}-{counter:x}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn unique_ids_differ() {
        let first = super::unique_id();
        let second = super::unique_id();
        assert_ne!(first, second);
    }
}

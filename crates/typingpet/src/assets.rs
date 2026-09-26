//! 内置图片素材：直接嵌入二进制，首次运行时释放到数据目录。
//!
//! 素材来源与授权见仓库根目录 `ASSET_LICENSE.md`（CC0 1.0）。

use std::fs;
use std::path::Path;

pub const BUILT_IN_ASSETS: &[(&str, &[u8])] = &[
    (
        "pet-idle.png",
        include_bytes!("../resources/pet-idle.png"),
    ),
    (
        "pet-left.png",
        include_bytes!("../resources/pet-left.png"),
    ),
    (
        "pet-right.png",
        include_bytes!("../resources/pet-right.png"),
    ),
    (
        "pet-question.png",
        include_bytes!("../resources/pet-question.png"),
    ),
    (
        "pet-exclamation.png",
        include_bytes!("../resources/pet-exclamation.png"),
    ),
];

/// 把内置素材释放到 `directory`（已存在则跳过）。
pub fn ensure_builtin_assets(directory: &Path) -> std::io::Result<()> {
    fs::create_dir_all(directory)?;
    for (name, bytes) in BUILT_IN_ASSETS {
        let path = directory.join(name);
        if !path.is_file() {
            fs::write(&path, bytes)?;
        }
    }
    Ok(())
}

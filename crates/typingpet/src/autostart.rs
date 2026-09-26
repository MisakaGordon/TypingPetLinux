//! 开机自启：写/删 `~/.config/autostart/typingpet.desktop`。
//!
//! 与 macOS 的 `SMAppService` 等价的最小实现；`Exec` 指向当前可执行文件，
//! 因此在 `cargo run` 与安装后运行都能正确指向对应二进制。

use std::fs;
use std::path::PathBuf;

pub const DESKTOP_FILE_NAME: &str = "typingpet.desktop";

pub fn autostart_directory() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("autostart")
}

pub fn autostart_path() -> PathBuf {
    autostart_directory().join(DESKTOP_FILE_NAME)
}

pub fn is_enabled() -> bool {
    autostart_path().is_file()
}

pub fn set_enabled(enabled: bool) -> std::io::Result<()> {
    let path = autostart_path();
    if !enabled {
        if path.exists() {
            fs::remove_file(&path)?;
        }
        return Ok(());
    }

    let executable = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("typingpet"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let contents = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=TypingPet\n\
         Comment=A desktop pet that reacts to your typing\n\
         Exec=\"{}\"\n\
         Icon=input-keyboard\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n\
         Categories=Utility;\n",
        executable.display()
    );
    fs::write(&path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autostart_file_is_created_and_removed() {
        let root = std::env::temp_dir().join(format!("typingpet-autostart-{}", std::process::id()));
        std::env::set_var("XDG_CONFIG_HOME", &root);
        let path = autostart_path();
        assert_eq!(path, root.join("autostart").join(DESKTOP_FILE_NAME));

        set_enabled(true).expect("enable");
        assert!(is_enabled());
        let contents = fs::read_to_string(&path).expect("read");
        assert!(contents.contains("Exec="));
        assert!(contents.contains("Type=Application"));

        set_enabled(false).expect("disable");
        assert!(!is_enabled());

        fs::remove_dir_all(&root).ok();
    }
}

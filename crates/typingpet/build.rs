//! 打包相关：给二进制加一条相对 rpath。
//!
//! 安装后的布局是 `<prefix>/bin/typingpet` + `<prefix>/lib/libgtk4-layer-shell.so.0`，
//! 所以需要 `$ORIGIN/../lib` —— 这样在没有 gtk4-layer-shell 的发行版上，
//! 随包附带的那份能被找到；系统里有的话（安装脚本会跳过附带的那份）也照常工作。
fn main() {
    println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib");
    println!("cargo:rerun-if-changed=build.rs");
}

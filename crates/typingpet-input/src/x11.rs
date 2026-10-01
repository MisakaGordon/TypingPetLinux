//! X11 只读查询：全局光标位置与屏幕尺寸。
//!
//! 使用 x11rb 的纯 Rust 后端，不需要 libX11 头文件；未连接 X11 时全部返回 `None`。

use crate::{PointerPosition, ScreenSize};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt;

fn connect() -> Option<(impl Connection, usize)> {
    x11rb::connect(None).ok()
}

/// 是否有可用的 X11 连接。
pub fn is_available() -> bool {
    connect().is_some()
}

pub fn pointer_position() -> Option<PointerPosition> {
    let (connection, screen_number) = connect()?;
    let root = connection.setup().roots.get(screen_number)?.root;
    let reply = connection.query_pointer(root).ok()?.reply().ok()?;
    Some(PointerPosition {
        x: f64::from(reply.root_x),
        y: f64::from(reply.root_y),
    })
}

/// 把 X11 光标移动到屏幕绝对坐标。
///
/// 仅用于本地测试悬停/躲避逻辑（Wayland 下协议不允许，只能在 X11 会话里用）。
/// 注意：这会真的移动光标。
pub fn warp_pointer(x: i16, y: i16) -> Option<()> {
    let (connection, screen_number) = connect()?;
    let root = connection.setup().roots.get(screen_number)?.root;
    connection
        .warp_pointer(x11rb::NONE, root, 0, 0, 0, 0, x, y)
        .ok()?;
    connection.flush().ok()?;
    Some(())
}

pub fn screen_size() -> Option<ScreenSize> {
    let (connection, screen_number) = connect()?;
    let screen = connection.setup().roots.get(screen_number)?;
    Some(ScreenSize {
        width: f64::from(screen.width_in_pixels),
        height: f64::from(screen.height_in_pixels),
    })
}

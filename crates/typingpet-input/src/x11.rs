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

pub fn screen_size() -> Option<ScreenSize> {
    let (connection, screen_number) = connect()?;
    let screen = connection.setup().roots.get(screen_number)?;
    Some(ScreenSize {
        width: f64::from(screen.width_in_pixels),
        height: f64::from(screen.height_in_pixels),
    })
}

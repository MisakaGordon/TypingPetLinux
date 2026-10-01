//! X11 只读查询 + 窗口移动。
//!
//! 用 x11rb 的纯 Rust 后端，不需要 libX11 头文件；未连接 X11 时全部返回 `None`。
//!
//! 连接是**复用**的：光标轮询是 30Hz，如果每次都重连会白白创建大量连接。

use crate::{PointerPosition, ScreenSize};
use std::cell::RefCell;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConfigureWindowAux, ConnectionExt};
use x11rb::rust_connection::RustConnection;

thread_local! {
    static CONNECTION: RefCell<Option<(RustConnection, usize)>> = const { RefCell::new(None) };
}

/// 在复用的连接上执行操作；首次调用时建立连接。
fn with_connection<T>(action: impl FnOnce(&RustConnection, usize) -> T) -> Option<T> {
    CONNECTION.with(|cell| {
        if cell.borrow().is_none() {
            match x11rb::connect(None) {
                Ok(pair) => *cell.borrow_mut() = Some(pair),
                Err(_) => return None,
            }
        }
        let borrowed = cell.borrow();
        let (connection, screen_number) = borrowed.as_ref()?;
        Some(action(connection, *screen_number))
    })
}

/// 是否有可用的 X11 连接。
pub fn is_available() -> bool {
    with_connection(|_, _| ()).is_some()
}

pub fn pointer_position() -> Option<PointerPosition> {
    with_connection(|connection, screen_number| {
        let root = connection.setup().roots.get(screen_number)?.root;
        let reply = connection.query_pointer(root).ok()?.reply().ok()?;
        Some(PointerPosition {
            x: f64::from(reply.root_x),
            y: f64::from(reply.root_y),
        })
    })
    .flatten()
}

pub fn screen_size() -> Option<ScreenSize> {
    with_connection(|connection, screen_number| {
        let screen = connection.setup().roots.get(screen_number)?;
        Some(ScreenSize {
            width: f64::from(screen.width_in_pixels),
            height: f64::from(screen.height_in_pixels),
        })
    })
    .flatten()
}

/// 把 X11 光标移动到屏幕绝对坐标。
///
/// 仅用于本地测试悬停/躲避（Wayland 协议不允许移动光标）。
/// 注意：在 Wayland 会话里经 XWayland 运行时，KWin 会忽略这个请求（调用不报错但光标不动）。
pub fn warp_pointer(x: i16, y: i16) -> Option<()> {
    with_connection(|connection, screen_number| {
        let root = connection.setup().roots.get(screen_number)?.root;
        connection
            .warp_pointer(x11rb::NONE, root, 0, 0, 0, 0, x, y)
            .ok()?;
        connection.flush().ok()?;
        Some(())
    })
    .flatten()
}

/// 移动一个已映射的 X11 窗口（坐标：root 坐标系，物理像素）。
///
/// GTK4 没有任何窗口定位 API，X11 下只能直接发 `ConfigureWindow`；
/// layer-shell 那条路（`set_margin`）只在 Wayland 下可用。
pub fn move_window(window_id: u32, x: i32, y: i32) -> bool {
    with_connection(|connection, _| {
        let request = ConfigureWindowAux::new().x(x).y(y);
        connection.configure_window(window_id, &request).is_ok()
            && connection.flush().is_ok()
    })
    .unwrap_or(false)
}

/// 读回窗口在 root 坐标系里的真实位置与尺寸（用于自检/日志）。
pub fn window_geometry(window_id: u32) -> Option<(i32, i32, u32, u32)> {
    with_connection(|connection, _| {
        let geometry = connection.get_geometry(window_id).ok()?.reply().ok()?;
        let translated = connection
            .translate_coordinates(window_id, geometry.root, 0, 0)
            .ok()?
            .reply()
            .ok()?;
        Some((
            i32::from(translated.dst_x),
            i32::from(translated.dst_y),
            u32::from(geometry.width),
            u32::from(geometry.height),
        ))
    })
    .flatten()
}

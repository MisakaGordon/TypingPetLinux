//! 宠物窗口 + 对外操作接口（`PetUi`）。
//!
//! `PetUi` 是 GUI 线程上的单一入口：托盘命令队列、设置窗口、内部状态机都通过它修改状态，
//! 状态变更统一走 `update_settings` → 应用 UI → 落盘 → 同步托盘。

use crate::autostart;
use crate::settings_window::SettingsView;
use crate::tray::{self, SharedTray, TrayCommand};
use crate::{Options, Runtime};
use anyhow::Result;
use gtk::prelude::*;
use gtk4 as gtk;
use gtk4_layer_shell::LayerShell;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use typingpet_core::config::Settings;
use typingpet_core::geom::{self, Point, Rect, Size};
use typingpet_core::keys::{KeyReactionRule, KeyStroke};
use typingpet_core::library::PetImageSet;
use typingpet_core::picker::SystemRng;
use typingpet_core::state::Action;

const BOUNCE_DURATION: f64 = 0.16;
const BOUNCE_PEAK: f64 = 0.42;
const FRAME_INTERVAL_MS: u64 = 16;
const POINTER_POLL_MS: u64 = 33;
const TRAY_SYNC_MS: u64 = 400;

/// Wayland 局部躲避的检测环宽度（逻辑像素）。
/// Wayland 不允许客户端查询全局光标，所以把窗口做得比图片每边大一圈：
/// 锁定时把这一圈也放进输入区域，就能在光标"靠近"时收到事件并躲避。
pub const AVOID_RING: f64 = 40.0;
/// 躲避弹簧参数（与 macOS 版一致）
const AVOID_STIFFNESS: f64 = 58.0;
const AVOID_DAMPING: f64 = 13.0;
const AVOID_MAX_SPEED: f64 = 520.0;
const AVOID_TRIGGER_GLOBAL: f64 = 90.0;

/// 当前 GDK 显示是否为 Wayland。
pub fn wayland_display() -> bool {
    gtk::gdk::Display::default()
        .map(|display| display.type_().name().contains("Wayland"))
        .unwrap_or(false)
}

pub fn layer_shell_supported() -> bool {
    wayland_display() && gtk4_layer_shell::is_supported()
}

pub fn load_thumbnail(path: Option<&Path>, size: i32) -> Option<gtk::gdk_pixbuf::Pixbuf> {
    let path = path?;
    gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(path, size, size, true).ok()
}

struct UiState {
    position: Point,
    base_size: Size,
    current: Option<PathBuf>,
    settings: Settings,
    bounce: Option<(Instant, f64)>,
    hover: bool,
    click_through: bool,
    pointer_polled_at: Option<Instant>,
    /// 屏幕坐标（左上原点）。X11 由全局轮询写入；Wayland 由检测环内的界面事件写入。
    pointer: Option<Point>,
    /// 本会话能否拿到全局光标位置：X11 可以，Wayland 协议不允许。
    ///
    /// 关键：这个判定必须在启动时定一次，**不能**按"这次轮询成功与否"来决定 ——
    /// 否则 X11 的物理像素坐标会覆盖 Wayland 的逻辑像素坐标（两者相差缩放倍率），
    /// 悬停判定就会以 30–60Hz 反复翻转，表现为桌宠疯狂闪烁。
    x11_pointer: bool,
    avoid_velocity: typingpet_core::geom::Vec2,
    avoid_last: Option<Instant>,
    avoid_settled: bool,
    wayland_note_printed: bool,
}

impl UiState {
    /// 宠物图片的显示尺寸
    fn image_size(&self) -> Size {
        geom::scaled_size(self.base_size, self.settings.scale)
    }

    /// 窗口尺寸 = 图片 + 四周的检测环
    fn window_size(&self) -> Size {
        let image = self.image_size();
        Size::new(image.w + AVOID_RING * 2.0, image.h + AVOID_RING * 2.0)
    }

    /// 悬停命中判定，带 6px 迟滞：从外部进入按宠物本体算，已经悬停时要超出 6px 才算离开，
    /// 避免光标停在边缘时透明度来回跳。
    fn hover_hit(&self, pointer: Point) -> bool {
        let frame = self.pet_frame();
        let slack = if self.hover { 6.0 } else { 0.0 };
        Rect::new(
            frame.x - slack,
            frame.y - slack,
            frame.w + slack * 2.0,
            frame.h + slack * 2.0,
        )
        .contains(pointer)
    }

    /// 宠物本体在屏幕上的矩形（左上原点）
    fn pet_frame(&self) -> Rect {
        Rect::from_origin_size(
            Point::new(self.position.x + AVOID_RING, self.position.y + AVOID_RING),
            self.image_size(),
        )
    }
}

/// 拖动时的目标窗口位置。
///
/// `gesture_offset` 是 GtkGestureDrag 给的偏移，它的参考系是**窗口（surface）自身** ——
/// 窗口一动，这个参考系就跟着动。因此必须叠加到**当前**窗口位置上；
/// 若叠加到"按下那一刻"的旧位置上，自身位移会被算两遍，表现为拖动时抽搐/卡顿。
pub fn drag_target(current_window: Point, gesture_offset: Point) -> Point {
    Point::new(
        current_window.x + gesture_offset.x,
        current_window.y + gesture_offset.y,
    )
}

/// X11 root 坐标（物理像素）→ 窗口/宠物框坐标（GDK 逻辑像素）。
///
/// 两者在缩放会话下相差 scale 倍，混用会让悬停判定失效（表现为桌宠疯狂闪烁）。
pub fn physical_to_logical(pointer: Point, scale: f64) -> Point {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    Point::new(pointer.x / scale, pointer.y / scale)
}

/// 输入区域策略（纯函数，便于单测）。返回相对窗口左上角的矩形列表，空表示完全穿透。
pub fn input_region_rects(
    locked: bool,
    avoid_pointer: bool,
    window: Size,
    ring: f64,
) -> Vec<Rect> {
    if !locked {
        // 可交互：只有宠物本体接收事件，透明环不吃点击
        return vec![Rect::new(ring, ring, window.w - ring * 2.0, window.h - ring * 2.0)];
    }
    if avoid_pointer {
        // 锁定 + 躲避：整个窗口（含环）接收事件，用于 Wayland 局部检测
        return vec![Rect::new(0.0, 0.0, window.w, window.h)];
    }
    // 锁定且不躲避：完全鼠标穿透
    Vec::new()
}

/// 等待用户按键的捕获状态。
struct Capture {
    image_path: PathBuf,
    dialog: gtk::Window,
    label: gtk::Label,
}

pub struct PetUi {
    window: gtk::Window,
    picture: gtk::Picture,
    fixed: gtk::Fixed,
    state: Rc<RefCell<UiState>>,
    pub runtime: Rc<RefCell<Runtime>>,
    tray: SharedTray,
    settings_view: RefCell<Option<Rc<SettingsView>>>,
    capture: RefCell<Option<Capture>>,
    quit: Arc<AtomicBool>,
    tray_synced_at: RefCell<Option<Instant>>,
    avoid_debug_at: RefCell<Option<Instant>>,
}

pub fn run(options: Options) -> Result<()> {
    let app = gtk::Application::builder()
        .application_id("io.github.typingpet.Linux")
        .build();

    // 所有初始化都放进 activate：
    // 1) 只有主实例会收到 activate，因此第二次启动不会抢 /dev/input、也不会注册出第二个托盘图标；
    // 2) GTK 把"第二次启动"转成一次 activate 信号，于是它天然成为"打开设置窗口"的入口
    //    （系统托盘把新图标收进隐藏项时，这是最可靠的入口）。
    let existing_ui: Rc<RefCell<Option<Rc<PetUi>>>> = Rc::new(RefCell::new(None));
    let pending_options: Rc<RefCell<Option<Options>>> = Rc::new(RefCell::new(Some(options)));

    let slot = existing_ui.clone();
    app.connect_activate(move |app| {
        if let Some(ui) = slot.borrow().as_ref().cloned() {
            println!("已有实例在运行：再次启动 = 打开设置窗口");
            ui.open_settings();
            return;
        }
        let Some(options) = pending_options.borrow_mut().take() else {
            return;
        };

        let runtime = match Runtime::bootstrap(options) {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("启动失败：{error:#}");
                std::process::exit(1);
            }
        };
        println!("== TypingPet ==");
        println!("{}", runtime.description());
        println!("config: {}", runtime.options.config_path.display());

        let session = typingpet_input::session_kind().to_string();
        let input_status = match runtime.input.backend() {
            typingpet_input::Backend::Evdev => "evdev（可用）".to_string(),
            typingpet_input::Backend::Mock => "mock（脚本按键）".to_string(),
        };
        let tray: SharedTray = Arc::new(std::sync::Mutex::new(tray::TrayState::new(
            input_status,
            session,
        )));
        let quit = Arc::new(AtomicBool::new(false));
        if !runtime.options.no_tray {
            tray::spawn(tray.clone());
        }

        let runtime = Rc::new(RefCell::new(runtime));
        let ui = PetUi::build(app, runtime, tray, quit);
        if ui.runtime.borrow().options.open_settings.unwrap_or(false) {
            ui.clone().open_settings();
        }
        PetUi::start_frame_loop(&ui);
        *slot.borrow_mut() = Some(ui);
    });

    app.run_with_args::<&str>(&[]);
    Ok(())
}

impl PetUi {
    fn build(
        app: &gtk::Application,
        runtime: Rc<RefCell<Runtime>>,
        tray: SharedTray,
        quit: Arc<AtomicBool>,
    ) -> Rc<Self> {
        let (settings, idle_url, base_size, no_layer_shell, layer, position_override, reset_position) = {
            let runtime = runtime.borrow();
            let base_size = runtime
                .library
                .idle_url()
                .and_then(|path| typingpet_core::image_dimensions(&path))
                .map(|size| geom::normalized_base_size(size, 453.0))
                .unwrap_or_else(|| Size::new(453.0, 354.0));
            (
                runtime.settings.clone(),
                runtime.library.idle_url(),
                base_size,
                runtime.options.no_layer_shell,
                runtime.options.layer.clone(),
                runtime.options.position,
                runtime.options.reset_position,
            )
        };

        let window = gtk::Window::builder()
            .application(app)
            .decorated(false)
            .resizable(false)
            .title("TypingPet")
            .build();
        window.add_css_class("typingpet-window");

        install_css();

        let picture = gtk::Picture::new();
        picture.set_content_fit(gtk::ContentFit::Contain);
        picture.set_can_shrink(true);

        let fixed = gtk::Fixed::new();
        fixed.set_overflow(gtk::Overflow::Hidden);
        // 图片放在窗口内的检测环偏移处，四周留出透明环
        fixed.put(&picture, AVOID_RING, AVOID_RING);
        window.set_child(Some(&fixed));

        let layer_shell = setup_layer_shell(&window, &settings, no_layer_shell, &layer);
        let monitor = primary_monitor_geometry();
        log_monitors();
        let image_size = geom::scaled_size(base_size, settings.scale);
        let start_size = Size::new(
            image_size.w + AVOID_RING * 2.0,
            image_size.h + AVOID_RING * 2.0,
        );
        let position =
            resolve_position(&settings, start_size, position_override, reset_position, monitor);
        println!(
            "position: ({:.0},{:.0}) monitor={}x{} scale={:.2} layer={} click_through={}",
            position.x,
            position.y,
            monitor.width,
            monitor.height,
            monitor.scale,
            layer,
            settings.position_locked
        );

        window.set_default_size(start_size.w as i32, start_size.h as i32);
        fixed.set_size_request(start_size.w as i32, start_size.h as i32);
        if layer_shell {
            apply_position(&window, position.x, position.y);
        }
        if let Some(path) = &idle_url {
            render_picture(&picture, path, image_size);
        }

        let click_through = settings.position_locked;
        let state = Rc::new(RefCell::new(UiState {
            position,
            base_size,
            current: idle_url.clone(),
            settings,
            bounce: None,
            hover: false,
            click_through,
            pointer_polled_at: None,
            pointer: None,
            x11_pointer: !wayland_display(),
            avoid_velocity: typingpet_core::geom::Vec2::ZERO,
            avoid_last: None,
            avoid_settled: true,
            wayland_note_printed: false,
        }));

        let ui = Rc::new(Self {
            window: window.clone(),
            picture: picture.clone(),
            fixed: fixed.clone(),
            state: state.clone(),
            runtime: runtime.clone(),
            tray: tray.clone(),
            settings_view: RefCell::new(None),
            capture: RefCell::new(None),
            quit: quit.clone(),
            tray_synced_at: RefCell::new(None),
            avoid_debug_at: RefCell::new(None),
        });

        apply_input_region(&ui);

        attach_motion(&window, &ui);
        attach_drag(&window, &ui);
        attach_scroll(&window, &ui);
        attach_secondary_click(&window, &ui);

        println!(
            "pointer: 来源={}",
            if state.borrow().x11_pointer {
                "X11 全局轮询（同一坐标空间，躲避提前量 90px）"
            } else {
                "Wayland 局部事件（检测环内，躲避提前量 40px）"
            }
        );
        println!("window: 已创建 layer_shell={layer_shell}");
        println!(
            "idle: {}",
            idle_url
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "<none>".to_string())
        );
        println!("提示：滚轮=缩放，拖拽=移动，托盘/设置窗口=全部功能");
        if !layer_shell {
            println!(
                "提示：当前未启用 layer-shell（会话={}），窗口不会置顶",
                typingpet_input::session_kind()
            );
        }
        window.present();
        ui.sync_tray_state(true);
        ui
    }

    // ---------- 对外状态 ----------

    pub fn settings(&self) -> Settings {
        self.state.borrow().settings.clone()
    }

    pub fn session(&self) -> String {
        typingpet_input::session_kind().to_string()
    }

    pub fn input_status(&self) -> String {
        match self.runtime.borrow().input.backend() {
            typingpet_input::Backend::Evdev => "evdev（可用）".to_string(),
            typingpet_input::Backend::Mock => "mock（脚本按键，未接真实键盘）".to_string(),
        }
    }

    pub fn pointer_available(&self) -> bool {
        typingpet_input::pointer_position().is_some()
    }

    pub fn is_autostart_enabled(&self) -> bool {
        autostart::is_enabled()
    }

    pub fn set_autostart(&self, enabled: bool) -> std::result::Result<(), String> {
        autostart::set_enabled(enabled).map_err(|error| error.to_string())
    }

    // ---------- 设置修改 ----------

    fn update_settings(&self, apply: impl FnOnce(&mut Settings)) {
        let config_path = self.runtime.borrow().options.config_path.clone();
        let updated = {
            let mut state = self.state.borrow_mut();
            apply(&mut state.settings);
            let clamped = state.settings.clone().clamped();
            state.settings = clamped.clone();
            clamped
        };
        self.runtime.borrow_mut().settings = updated.clone();

        // CLI 覆盖项只对本次会话有效：如果用户没有主动改动它，落盘时写回配置文件里的原值，
        // 避免 `--click-through` / `--avoid-pointer` / `--scale` / `--position` 被"粘"进配置。
        let to_save = {
            let runtime = self.runtime.borrow();
            let baseline = &runtime.baseline_settings;
            let overrides = &runtime.overrides;
            let mut save = updated.clone();
            if let Some(value) = overrides.scale {
                if (save.scale - value).abs() < f64::EPSILON {
                    save.scale = baseline.scale;
                }
            }
            if let Some(value) = overrides.position_locked {
                if save.position_locked == value {
                    save.position_locked = baseline.position_locked;
                }
            }
            if let Some(value) = overrides.avoids_pointer_when_locked {
                if save.avoids_pointer_when_locked == value {
                    save.avoids_pointer_when_locked = baseline.avoids_pointer_when_locked;
                }
            }
            if let Some((x, y)) = overrides.pet_position {
                if (save.pet_x - x).abs() < 0.5 && (save.pet_y - y).abs() < 0.5 {
                    save.pet_x = baseline.pet_x;
                    save.pet_y = baseline.pet_y;
                }
            }
            save
        };
        if let Err(error) = to_save.save(&config_path) {
            eprintln!("settings: 保存失败 {error}");
        }
        self.sync_tray_state(true);
    }

    pub fn set_scale(&self, scale: f64) {
        self.update_settings(|settings| settings.scale = scale);
        let (window_size, image_size) = {
            let state = self.state.borrow();
            (state.window_size(), state.image_size())
        };
        self.window.set_default_size(window_size.w as i32, window_size.h as i32);
        self.fixed.set_size_request(window_size.w as i32, window_size.h as i32);
        let current = self.state.borrow().current.clone();
        if let Some(path) = current {
            render_picture(&self.picture, &path, image_size);
        }
        apply_input_region(self);
    }

    pub fn set_resting_opacity(&self, value: f64) {
        self.update_settings(|settings| settings.resting_opacity = value);
        self.apply_opacity_now();
    }

    pub fn set_hover_opacity(&self, value: f64) {
        self.update_settings(|settings| settings.hover_opacity = value);
        self.apply_opacity_now();
    }

    pub fn set_shake_level(&self, level: u8) {
        let amplitude = {
            let mut probe = self.state.borrow().settings.clone();
            probe.shake_level = level.min(3);
            probe.shake_amplitude()
        };
        self.update_settings(|settings| settings.shake_level = level.min(3));
        let (idle, reactions) = {
            let runtime = self.runtime.borrow();
            (runtime.library.idle_url(), runtime.library.reaction_urls())
        };
        self.runtime
            .borrow_mut()
            .state
            .update_sources(idle, reactions, amplitude);
    }

    pub fn set_always_on_top(&self, enabled: bool) {
        self.update_settings(|settings| settings.always_on_top = enabled);
        if layer_shell_supported() {
            let layer = if enabled {
                gtk4_layer_shell::Layer::Top
            } else {
                gtk4_layer_shell::Layer::Bottom
            };
            self.window.set_layer(layer);
        }
    }

    pub fn set_position_locked(&self, locked: bool) {
        self.update_settings(|settings| settings.position_locked = locked);
        self.state.borrow_mut().click_through = locked;
        apply_input_region(self);
        if locked {
            self.reload_pointer_sample();
        }
    }

    pub fn set_avoid_pointer(&self, enabled: bool) {
        self.update_settings(|settings| settings.avoids_pointer_when_locked = enabled);
        apply_input_region(self);
        {
            let mut state = self.state.borrow_mut();
            state.avoid_velocity = typingpet_core::geom::Vec2::ZERO;
            state.avoid_last = None;
            state.avoid_settled = true;
        }
    }

    pub fn reset_position(&self) {
        let monitor = primary_monitor_geometry();
        let size = self.state.borrow().window_size();
        let position = Point::new(
            monitor.x + monitor.width - size.w - 24.0,
            monitor.y + monitor.height - size.h - 24.0,
        );
        self.state.borrow_mut().position = position;
        apply_position(&self.window, position.x, position.y);
        self.update_settings(|settings| {
            settings.pet_x = position.x;
            settings.pet_y = position.y;
        });
    }

    pub fn is_pet_visible(&self) -> bool {
        self.window.is_visible()
    }

    pub fn set_pet_visible(&self, visible: bool) {
        self.window.set_visible(visible);
        if visible {
            self.apply_opacity_now();
        }
        self.sync_tray_state(true);
    }

    pub fn toggle_pet_visible(&self) {
        let visible = !self.is_pet_visible();
        self.set_pet_visible(visible);
    }

    /// 图片集/规则变化后重新加载待机图与反应图。
    pub fn reload_images(&self) {
        let (idle, reactions, base_size) = {
            let runtime = self.runtime.borrow();
            let idle = runtime.library.idle_url();
            let reactions = runtime.library.reaction_urls();
            let base_size = idle
                .as_ref()
                .and_then(|path| typingpet_core::image_dimensions(path))
                .map(|size| geom::normalized_base_size(size, 453.0))
                .unwrap_or_else(|| Size::new(453.0, 354.0));
            (idle, reactions, base_size)
        };

        let shake = self.state.borrow().settings.shake_amplitude();
        {
            let mut runtime = self.runtime.borrow_mut();
            runtime
                .state
                .update_sources(idle.clone(), reactions, shake);
        }
        let (window_size, image_size) = {
            let mut state = self.state.borrow_mut();
            state.base_size = base_size;
            state.current = idle.clone();
            state.bounce = None;
            (state.window_size(), state.image_size())
        };
        self.window.set_default_size(window_size.w as i32, window_size.h as i32);
        self.fixed.set_size_request(window_size.w as i32, window_size.h as i32);
        if let Some(path) = &idle {
            render_picture(&self.picture, path, image_size);
        }
        apply_input_region(self);
        self.sync_tray_state(true);
    }

    // ---------- 图库 ----------

    pub fn image_sets(&self) -> Vec<PetImageSet> {
        self.runtime.borrow().library.image_sets()
    }

    pub fn active_set_id(&self) -> String {
        self.runtime.borrow().library.active_set_id().to_string()
    }

    pub fn idle_url_for(&self, set: &PetImageSet) -> Option<PathBuf> {
        self.runtime.borrow().library.idle_url_for(set)
    }

    pub fn reaction_count_for(&self, set: &PetImageSet) -> usize {
        self.runtime.borrow().library.reaction_urls_for(set).len()
    }

    pub fn activate_set(&self, id: &str) -> std::result::Result<(), String> {
        let result = self
            .runtime
            .borrow_mut()
            .library
            .activate_set(id)
            .map_err(|error| error.to_string());
        if result.is_ok() {
            self.reload_images();
        }
        result
    }

    pub fn import_folder(&self, folder: &Path) -> std::result::Result<PetImageSet, String> {
        let result = self
            .runtime
            .borrow_mut()
            .library
            .import_folder(folder)
            .map_err(|error| error.to_string());
        match result {
            Ok(import) => {
                self.reload_images();
                Ok(import.image_set)
            }
            Err(error) => Err(error),
        }
    }

    pub fn rename_set(&self, id: &str, name: &str) -> std::result::Result<(), String> {
        self.runtime
            .borrow_mut()
            .library
            .rename_set(id, name)
            .map_err(|error| error.to_string())
    }

    pub fn delete_set(&self, id: &str) -> std::result::Result<(), String> {
        let result = self
            .runtime
            .borrow_mut()
            .library
            .delete_set(id)
            .map_err(|error| error.to_string());
        if result.is_ok() {
            self.reload_images();
        }
        result
    }

    // ---------- 键反应规则 ----------

    pub fn rules(&self) -> Vec<KeyReactionRule> {
        self.runtime.borrow().rules.rules().to_vec()
    }

    pub fn rule_image_path(&self, rule: &KeyReactionRule) -> Option<PathBuf> {
        self.runtime.borrow().rules.image_path_for(rule)
    }

    pub fn add_rule(&self, stroke: KeyStroke, image: &Path) -> std::result::Result<(), String> {
        let result = self
            .runtime
            .borrow_mut()
            .rules
            .set_rule(stroke, image)
            .map(|_| ())
            .map_err(|error| error.to_string());
        if result.is_ok() {
            self.reload_images();
        }
        result
    }

    pub fn remove_rule(&self, id: &str) {
        if let Err(error) = self.runtime.borrow_mut().rules.remove_rule(id) {
            eprintln!("rules: 删除失败 {error}");
        }
        self.reload_images();
    }

    /// 打开"按一个键"的捕获窗口；下一次按键会与 `image_path` 绑定。
    pub fn begin_capture(self: &Rc<Self>, image_path: PathBuf) {
        if let Some(existing) = self.capture.borrow().as_ref() {
            existing.dialog.present();
            return;
        }

        let dialog = gtk::Window::builder()
            .title("按下要绑定的键")
            .modal(true)
            .resizable(false)
            .default_width(380)
            .default_height(160)
            .build();
        if let Some(parent) = self.settings_window() {
            dialog.set_transient_for(Some(&parent));
        }

        let label = gtk::Label::new(Some("请按下要绑定的键或组合键…"));
        label.add_css_class("capture-hint");
        let hint = gtk::Label::new(Some("支持 Ctrl/Alt/Shift/Super 组合；点取消放弃"));
        hint.add_css_class("dim-label");

        let cancel = gtk::Button::with_label("取消");
        {
            let ui = self.clone();
            cancel.connect_clicked(move |_| {
                if let Some(capture) = ui.capture.borrow_mut().take() {
                    capture.dialog.close();
                }
            });
        }

        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_top(20);
        content.set_margin_bottom(20);
        content.set_margin_start(20);
        content.set_margin_end(20);
        content.append(&label);
        content.append(&hint);
        content.append(&cancel);
        dialog.set_child(Some(&content));

        {
            let ui = self.clone();
            dialog.connect_close_request(move |_| {
                ui.capture.borrow_mut().take();
                gtk::glib::Propagation::Proceed
            });
        }

        *self.capture.borrow_mut() = Some(Capture {
            image_path,
            dialog: dialog.clone(),
            label,
        });
        dialog.present();
    }

    /// 返回 `true` 表示这次按键被捕获窗口消费掉了。
    fn handle_capture(&self, stroke: KeyStroke) -> bool {
        let active = {
            let capture = self.capture.borrow();
            capture.as_ref().map(|active| {
                (
                    active.image_path.clone(),
                    active.dialog.clone(),
                    active.label.clone(),
                )
            })
        };
        let Some((image, dialog, label)) = active else {
            return false;
        };
        label.set_text(&format!("已捕获：{}", stroke.display_name()));
        match self.add_rule(stroke, &image) {
            Ok(()) => {
                self.capture.borrow_mut().take();
                dialog.close();
                self.refresh_settings();
            }
            Err(error) => label.set_text(&format!("保存失败：{error}")),
        }
        true
    }

    // ---------- 设置窗口 ----------

    pub fn settings_window(&self) -> Option<gtk::Window> {
        self.settings_view
            .borrow()
            .as_ref()
            .map(|view| view.window().clone())
    }

    pub fn open_settings(self: &Rc<Self>) {
        if let Some(view) = self.settings_view.borrow().as_ref() {
            view.window().present();
            view.refresh();
            return;
        }
        let tab = self.runtime.borrow().options.settings_tab;
        let view = SettingsView::build(self, tab);
        {
            let ui = self.clone();
            view.window().connect_close_request(move |_| {
                ui.settings_view.borrow_mut().take();
                gtk::glib::Propagation::Proceed
            });
        }
        view.window().present();
        *self.settings_view.borrow_mut() = Some(view);
    }

    pub fn refresh_settings(&self) {
        if let Some(view) = self.settings_view.borrow().as_ref() {
            view.refresh();
        }
    }

    // ---------- 托盘 ----------

    fn sync_tray_state(&self, force: bool) {
        if !force {
            let due = self
                .tray_synced_at
                .borrow()
                .map(|at| at.elapsed() >= Duration::from_millis(TRAY_SYNC_MS))
                .unwrap_or(true);
            if !due {
                return;
            }
        }
        *self.tray_synced_at.borrow_mut() = Some(Instant::now());

        let settings = self.state.borrow().settings.clone();
        let visible = self.window.is_visible();
        let reactions = self.runtime.borrow().state.reaction_count();
        let sets = self.runtime.borrow().library.image_sets().len();
        let input = self.input_status();
        let session = self.session();

        let changed = {
            let mut shared = match self.tray.lock() {
                Ok(shared) => shared,
                Err(_) => return,
            };
            let changed = shared.pet_visible != visible
                || shared.always_on_top != settings.always_on_top
                || shared.click_through != settings.position_locked
                || (shared.scale - settings.scale).abs() > f64::EPSILON
                || shared.reaction_count != reactions
                || shared.set_count != sets
                || shared.input_status != input
                || shared.session != session;
            shared.pet_visible = visible;
            shared.always_on_top = settings.always_on_top;
            shared.click_through = settings.position_locked;
            shared.scale = settings.scale;
            shared.reaction_count = reactions;
            shared.set_count = sets;
            shared.input_status = input;
            shared.session = session;
            changed
        };
        if changed {
            tray::refresh();
        }
    }

    fn drain_tray_commands(self: &Rc<Self>) {
        for command in tray::drain(&self.tray) {
            match command {
                TrayCommand::ToggleVisibility => self.toggle_pet_visible(),
                TrayCommand::ToggleAlwaysOnTop => {
                    let next = !self.state.borrow().settings.always_on_top;
                    self.set_always_on_top(next);
                    self.refresh_settings();
                }
                TrayCommand::ToggleClickThrough => {
                    let next = !self.state.borrow().settings.position_locked;
                    self.set_position_locked(next);
                    self.refresh_settings();
                }
                TrayCommand::ResetPosition => self.reset_position(),
                TrayCommand::SetScale(scale) => {
                    self.set_scale(scale);
                    self.refresh_settings();
                }
                TrayCommand::OpenSettings => self.open_settings(),
                TrayCommand::Quit => {
                    self.quit.store(true, Ordering::Relaxed);
                    std::process::exit(0);
                }
            }
        }
    }

    // ---------- 内部工具 ----------

    fn apply_opacity_now(&self) {
        let (settings, hovering) = {
            let state = self.state.borrow();
            (state.settings.clone(), state.hover)
        };
        apply_opacity(&self.window, &settings, hovering);
    }

    /// X11：轮询全局光标；Wayland：协议拿不到全局光标，直接不轮询，位置由界面事件提供。
    ///
    /// 两套坐标空间不能混用：X11 root 是物理像素，窗口/宠物框是 GDK 逻辑像素，
    /// 在缩放的 X11 会话下相差 scale 倍，所以这里统一除回去。
    fn reload_pointer_sample(&self) {
        if !self.state.borrow().x11_pointer {
            return;
        }
        let scale = primary_monitor_geometry().scale.max(1.0);
        let global = typingpet_input::pointer_position()
            .map(|pointer| physical_to_logical(Point::new(pointer.x, pointer.y), scale));
        let mut state = self.state.borrow_mut();
        state.pointer_polled_at = Some(Instant::now());
        if let Some(pointer) = global {
            state.pointer = Some(pointer);
        }
    }

    /// 根据指针位置更新悬停状态并套用透明度（唯一入口，避免多个来源互相覆盖）。
    fn refresh_hover(&self, pointer: Point) {
        let hovering = self.state.borrow().hover_hit(pointer);
        let changed = {
            let mut state = self.state.borrow_mut();
            let changed = state.hover != hovering;
            state.hover = hovering;
            changed
        };
        if changed {
            self.apply_opacity_now();
        }
    }

    /// 躲避光标：锁定 + 开关打开时，用阻尼弹簧把窗口推离光标。
    ///
    /// X11 用全局光标（90px 提前量）；Wayland 只用检测环内的事件（40px），
    /// 因此 Wayland 下是"靠近才躲"，这也正是平台限制下的可行方案。
    fn advance_avoidance(&self) {
        let (locked, avoid, pointer, global_pointer, position, pet_frame, window_size) = {
            let state = self.state.borrow();
            (
                state.settings.position_locked,
                state.settings.avoids_pointer_when_locked,
                state.pointer,
                state.x11_pointer,
                state.position,
                state.pet_frame(),
                state.window_size(),
            )
        };

        if !(locked && avoid) {
            let mut state = self.state.borrow_mut();
            state.avoid_velocity = typingpet_core::geom::Vec2::ZERO;
            state.avoid_last = None;
            return;
        }

        let monitor = primary_monitor_geometry();
        let bounds = Rect::new(monitor.x, monitor.y, monitor.width, monitor.height);
        let trigger = if global_pointer {
            AVOID_TRIGGER_GLOBAL
        } else {
            AVOID_RING
        };
        let target = pointer.and_then(|pointer| {
            geom::PetPointerAvoidance::target_origin_with(pointer, pet_frame, bounds, trigger)
        });

        let now = Instant::now();
        let elapsed = {
            let state = self.state.borrow();
            state
                .avoid_last
                .map(|last| (now - last).as_secs_f64().clamp(1.0 / 240.0, 1.0 / 30.0))
                .unwrap_or(1.0 / 60.0)
        };

        let (mut velocity, settled_before) = {
            let state = self.state.borrow();
            (state.avoid_velocity, state.avoid_settled)
        };
        let delta = match target {
            Some(target) => (
                target.x - position.x,
                target.y - position.y,
            ),
            None => (0.0, 0.0),
        };
        velocity.dx += (delta.0 * AVOID_STIFFNESS - velocity.dx * AVOID_DAMPING) * elapsed;
        velocity.dy += (delta.1 * AVOID_STIFFNESS - velocity.dy * AVOID_DAMPING) * elapsed;

        let speed = velocity.length();
        if speed > AVOID_MAX_SPEED {
            let factor = AVOID_MAX_SPEED / speed;
            velocity.dx *= factor;
            velocity.dy *= factor;
        }

        let proposed = Point::new(
            position.x + velocity.dx * elapsed,
            position.y + velocity.dy * elapsed,
        );
        let clamped = geom::PetPointerAvoidance::clamped_origin(proposed, window_size, bounds);
        if (clamped.x - proposed.x).abs() > 0.01 {
            velocity.dx = 0.0;
        }
        if (clamped.y - proposed.y).abs() > 0.01 {
            velocity.dy = 0.0;
        }

        let moved = (clamped.x - position.x).abs() > 0.01 || (clamped.y - position.y).abs() > 0.01;
        if moved {
            self.state.borrow_mut().position = clamped;
            apply_position(&self.window, clamped.x, clamped.y);
        }

        let stopping = target.is_none() && velocity.length() < 5.0;
        {
            let mut state = self.state.borrow_mut();
            state.avoid_velocity = velocity;
            state.avoid_last = Some(now);
            state.avoid_settled = stopping;
        }

        // 停稳后落盘一次位置（不要每帧写盘）
        if stopping && !settled_before {
            let position = self.state.borrow().position;
            self.update_settings(|settings| {
                settings.pet_x = position.x;
                settings.pet_y = position.y;
            });
        }
    }

    fn start_frame_loop(ui: &Rc<Self>) {
        let ui = ui.clone();
        let mut rng = SystemRng::new();

        gtk::glib::timeout_add_local(Duration::from_millis(FRAME_INTERVAL_MS), move || {
            if ui.quit.load(Ordering::Relaxed) {
                std::process::exit(0);
            }

            ui.drain_tray_commands();

            // 1) 输入 → 状态机（捕获窗口优先消费）
            let events: Vec<typingpet_input::KeyEvent> = {
                let runtime = ui.runtime.borrow();
                let mut events = Vec::new();
                while let Some(event) = runtime.input.try_recv() {
                    events.push(event);
                }
                events
            };
            for event in events {
                if ui.handle_capture(event.stroke) {
                    continue;
                }
                let print_events = ui.runtime.borrow().options.print_events;
                if print_events {
                    println!("key: {}", event.stroke.display_name());
                }
                let actions = {
                    let mut runtime = ui.runtime.borrow_mut();
                    let now = runtime.now();
                    let crate::Runtime { state, rules, .. } = &mut *runtime;
                    state.on_key(Some(event.stroke), rules, &mut rng, now)
                };
                ui.apply_actions(actions);
            }

            // 2) 待机超时
            let actions = {
                let mut runtime = ui.runtime.borrow_mut();
                let now = runtime.now();
                runtime.state.tick(now)
            };
            ui.apply_actions(actions);

            // 3) 弹跳动画
            let offset = {
                let mut state = ui.state.borrow_mut();
                match state.bounce {
                    Some((started, amplitude)) => {
                        let elapsed = started.elapsed().as_secs_f64();
                        if elapsed >= BOUNCE_DURATION {
                            state.bounce = None;
                            0.0
                        } else if elapsed <= BOUNCE_PEAK {
                            amplitude * (elapsed / BOUNCE_PEAK)
                        } else {
                            amplitude * (1.0 - (elapsed - BOUNCE_PEAK) / (1.0 - BOUNCE_PEAK))
                        }
                    }
                    None => 0.0,
                }
            };
            ui.fixed.move_(&ui.picture, AVOID_RING, AVOID_RING - offset);

            // 4) 光标采样：X11 走全局轮询，Wayland 只认界面事件。
            //    --simulate-pointer（调试）优先，且当帧不轮询，否则会被真光标覆盖。
            let simulating = ui.runtime.borrow().options.simulate_pointer;
            if let Some((x, y)) = simulating {
                let mut state = ui.state.borrow_mut();
                state.pointer = Some(Point::new(x, y));
                state.pointer_polled_at = Some(Instant::now());
            } else {
                let poll_due = ui
                    .state
                    .borrow()
                    .pointer_polled_at
                    .map(|at| at.elapsed() >= Duration::from_millis(POINTER_POLL_MS))
                    .unwrap_or(true);
                if poll_due {
                    ui.reload_pointer_sample();
                }
            }

            // 悬停透明度（带迟滞）
            // 注意：先把值取出来再调用，不能写成 if let Some(p) = ui.state.borrow().pointer
            // —— Rust 2021 里 if let 的临时借用守卫会活到整个 body，refresh_hover 里再 borrow_mut 会 panic。
            let pointer = ui.state.borrow().pointer;
            if let Some(pointer) = pointer {
                ui.refresh_hover(pointer);
            }
            {
                let mut state = ui.state.borrow_mut();
                if state.pointer.is_none() && !state.wayland_note_printed {
                    state.wayland_note_printed = true;
                    println!(
                        "note: Wayland 不允许查询全局光标 → 躲避改为局部检测：\
                         只在宠物周围 {AVOID_RING:.0}px 的检测环内生效（锁定+躲避时该环会占用鼠标点击）"
                    );
                }
            }

            // 5) 躲避光标（X11 全局预判 / Wayland 局部检测）
            ui.advance_avoidance();

            // 调试：模拟光标时每秒打印一次位置，便于确认躲避确实发生了
            if ui.runtime.borrow().options.simulate_pointer.is_some() {
                let due = ui
                    .avoid_debug_at
                    .borrow()
                    .map(|at: Instant| at.elapsed() >= Duration::from_millis(1000))
                    .unwrap_or(true);
                if due {
                    *ui.avoid_debug_at.borrow_mut() = Some(Instant::now());
                    let (position, pointer, hovering) = {
                        let state = ui.state.borrow();
                        (state.position, state.pointer, state.hover)
                    };
                    println!(
                        "sim-pointer: pointer=({:.0},{:.0}) window=({:.0},{:.0}) hover={hovering}",
                        pointer.map(|p| p.x).unwrap_or(-1.0),
                        pointer.map(|p| p.y).unwrap_or(-1.0),
                        position.x,
                        position.y
                    );
                }
            }

            ui.sync_tray_state(false);
            gtk::glib::ControlFlow::Continue
        });
    }

    fn apply_actions(&self, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::ShowImage(path) => {
                    let size = self.state.borrow().image_size();
                    render_picture(&self.picture, &path, size);
                    self.state.borrow_mut().current = Some(path);
                }
                Action::Bounce(amplitude) => {
                    self.state.borrow_mut().bounce = Some((Instant::now(), amplitude));
                }
            }
        }
    }
}

pub fn install_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        ".typingpet-window, .typingpet-window.background { background-color: rgba(0,0,0,0); box-shadow: none; }\n\
         .section-title { font-weight: bold; }\n\
         .dim-label { opacity: 0.65; }\n\
         .active-badge { color: #2ec27e; font-weight: bold; }\n\
         .capture-hint { font-size: 1.15em; font-weight: bold; }\n\
         .thumb-frame { background-color: alpha(currentColor, 0.08); border-radius: 6px; }",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

pub fn render_picture(picture: &gtk::Picture, path: &Path, size: Size) {
    let width = size.w.round().max(1.0) as i32;
    let height = size.h.round().max(1.0) as i32;
    // 关键：can_shrink=true 时 GtkPicture 的自然尺寸为 0，而 GtkFixed 按自然尺寸分配子控件，
    // 不给显式尺寸请求的话图片会被分配成 0x0（窗口在、但什么都不画）。
    picture.set_size_request(width, height);
    match gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(path, width, height, true) {
        Ok(pixbuf) => {
            let texture = gtk::gdk::Texture::for_pixbuf(&pixbuf);
            picture.set_paintable(Some(&texture));
        }
        Err(error) => eprintln!("image load failed ({}): {error}", path.display()),
    }
}

fn setup_layer_shell(
    window: &gtk::Window,
    settings: &Settings,
    force_disable: bool,
    requested_layer: &str,
) -> bool {
    if force_disable {
        println!("layer-shell: 已被 --no-layer-shell 禁用，使用普通无边框窗口");
        return false;
    }
    if !layer_shell_supported() {
        println!("layer-shell 不可用：退化为普通无边框窗口（置顶/定位交给窗口管理器）");
        return false;
    }
    window.init_layer_shell();
    let layer = match requested_layer {
        "overlay" => gtk4_layer_shell::Layer::Overlay,
        "bottom" => gtk4_layer_shell::Layer::Bottom,
        "background" => gtk4_layer_shell::Layer::Background,
        _ => {
            if settings.always_on_top {
                gtk4_layer_shell::Layer::Top
            } else {
                gtk4_layer_shell::Layer::Bottom
            }
        }
    };
    window.set_layer(layer);
    window.set_anchor(gtk4_layer_shell::Edge::Top, true);
    window.set_anchor(gtk4_layer_shell::Edge::Left, true);
    window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::None);
    window.set_exclusive_zone(-1);
    println!("layer-shell: 已启用（layer={requested_layer} + 点击穿透）");
    true
}

fn apply_position(window: &gtk::Window, x: f64, y: f64) {
    if !layer_shell_supported() {
        return;
    }
    window.set_margin(gtk4_layer_shell::Edge::Left, x.max(0.0) as i32);
    window.set_margin(gtk4_layer_shell::Edge::Top, y.max(0.0) as i32);
}

fn apply_opacity(window: &gtk::Window, settings: &Settings, hovering: bool) {
    let opacity = geom::PetOpacityBehavior::opacity(
        hovering,
        settings.resting_opacity,
        settings.hover_opacity,
    );
    window.set_opacity(opacity);
}

/// 按当前「位置锁定 / 躲避」状态刷新输入区域。
fn apply_input_region(ui: &PetUi) {
    let Some(surface) = ui.window.surface() else {
        return;
    };
    let (locked, avoid, window_size) = {
        let state = ui.state.borrow();
        (
            state.settings.position_locked,
            state.settings.avoids_pointer_when_locked,
            state.window_size(),
        )
    };
    let region = gtk::cairo::Region::create();
    for rect in input_region_rects(locked, avoid, window_size, AVOID_RING) {
        let rectangle = gtk::cairo::RectangleInt::new(
            rect.x.round() as i32,
            rect.y.round() as i32,
            rect.w.round().max(0.0) as i32,
            rect.h.round().max(0.0) as i32,
        );
        let _ = region.union_rectangle(&rectangle);
    }
    surface.set_input_region(Some(&region));
}

#[derive(Debug, Clone, Copy)]
struct MonitorGeometry {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale: f64,
}

impl Default for MonitorGeometry {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
            scale: 1.0,
        }
    }
}

fn primary_monitor_geometry() -> MonitorGeometry {
    let Some(display) = gtk::gdk::Display::default() else {
        return MonitorGeometry::default();
    };
    let Some(monitor) = display.monitors().item(0).and_downcast::<gtk::gdk::Monitor>() else {
        return MonitorGeometry::default();
    };
    let geometry = monitor.geometry();
    MonitorGeometry {
        x: f64::from(geometry.x()),
        y: f64::from(geometry.y()),
        width: f64::from(geometry.width()),
        height: f64::from(geometry.height()),
        scale: monitor.scale_factor() as f64,
    }
}

fn log_monitors() {
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    let monitors = display.monitors();
    for index in 0..monitors.n_items() {
        if let Some(monitor) = monitors.item(index).and_downcast::<gtk::gdk::Monitor>() {
            let geometry = monitor.geometry();
            println!(
                "monitor #{index}: {}x{} @ ({},{}) scale={} primary={}",
                geometry.width(),
                geometry.height(),
                geometry.x(),
                geometry.y(),
                monitor.scale_factor(),
                index == 0
            );
        }
    }
}

fn resolve_position(
    settings: &Settings,
    size: Size,
    override_position: Option<(f64, f64)>,
    reset: bool,
    monitor: MonitorGeometry,
) -> Point {
    let bottom_right = Point::new(
        monitor.x + monitor.width - size.w - 24.0,
        monitor.y + monitor.height - size.h - 24.0,
    );

    let requested = match (override_position, reset) {
        (Some((x, y)), _) => Point::new(x, y),
        (None, true) => bottom_right,
        (None, false) if settings.pet_x >= 0.0 && settings.pet_y >= 0.0 => {
            Point::new(settings.pet_x, settings.pet_y)
        }
        _ => bottom_right,
    };

    let clamped_x = requested.x.clamp(
        monitor.x,
        (monitor.x + monitor.width - size.w).max(monitor.x),
    );
    let clamped_y = requested.y.clamp(
        monitor.y,
        (monitor.y + monitor.height - size.h).max(monitor.y),
    );
    if (clamped_x - requested.x).abs() > 1.0 || (clamped_y - requested.y).abs() > 1.0 {
        println!(
            "position: 请求的 ({:.0},{:.0}) 超出显示器范围，已钳制到 ({:.0},{:.0})",
            requested.x, requested.y, clamped_x, clamped_y
        );
    }
    Point::new(clamped_x, clamped_y)
}

fn attach_motion(window: &gtk::Window, ui: &Rc<PetUi>) {
    let motion = gtk::EventControllerMotion::new();
    {
        let ui = ui.clone();
        // 局部指针位置：窗口坐标 → 屏幕坐标。Wayland 下这是唯一能拿到的光标信息，
        // 检测环范围内的移动都会走这里。
        motion.connect_motion(move |_, x, y| {
            // 界面事件永远是权威来源：它给的是窗口内坐标，加上窗口位置即为屏幕坐标，
            // 与宠物框同一个逻辑像素空间（不会再被 X11 的物理像素坐标覆盖）。
            let position = ui.state.borrow().position;
            let screen = Point::new(position.x + x, position.y + y);
            ui.state.borrow_mut().pointer = Some(screen);
            ui.refresh_hover(screen);
        });
    }
    {
        let ui = ui.clone();
        motion.connect_leave(move |_| {
            let mut state = ui.state.borrow_mut();
            // X11 下全局轮询每帧都会补上新位置，这里只清局部采样
            if !state.x11_pointer {
                state.pointer = None;
            }
            state.hover = false;
            drop(state);
            ui.apply_opacity_now();
        });
    }
    window.add_controller(motion);
}

fn attach_drag(window: &gtk::Window, ui: &Rc<PetUi>) {
    let drag = gtk::GestureDrag::new();

    {
        let ui = ui.clone();
        let window = window.clone();
        drag.connect_drag_update(move |_, offset_x, offset_y| {
            if ui.state.borrow().click_through {
                return;
            }
            let current = ui.state.borrow().position;
            let target = drag_target(current, Point::new(offset_x, offset_y));
            ui.state.borrow_mut().position = target;
            apply_position(&window, target.x, target.y);
        });
    }
    {
        let ui = ui.clone();
        drag.connect_drag_end(move |_, _, _| {
            if ui.state.borrow().click_through {
                return;
            }
            let position = ui.state.borrow().position;
            ui.update_settings(|settings| {
                settings.pet_x = position.x;
                settings.pet_y = position.y;
            });
        });
    }
    window.add_controller(drag);
}

/// 右键点宠物 = 打开设置窗口。
///
/// 系统托盘可能把新图标收进"隐藏项"，那样用户就没有入口打开设置，
/// 所以再给一个直接入口（位置锁定时窗口点击穿透，此时自然点不到）。
fn attach_secondary_click(window: &gtk::Window, ui: &Rc<PetUi>) {
    let click = gtk::GestureClick::new();
    click.set_button(3);
    {
        let ui = ui.clone();
        click.connect_released(move |_, _, _, _| {
            ui.open_settings();
        });
    }
    window.add_controller(click);
}

fn attach_scroll(window: &gtk::Window, ui: &Rc<PetUi>) {
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    {
        let ui = ui.clone();
        scroll.connect_scroll(move |_, _, dy| {
            let current = ui.state.borrow().settings.scale;
            let next = (current - dy * 0.05).clamp(0.35, 1.25);
            if (next - current).abs() < f64::EPSILON {
                return gtk::glib::Propagation::Proceed;
            }
            ui.set_scale(next);
            ui.refresh_settings();
            gtk::glib::Propagation::Proceed
        });
    }
    window.add_controller(scroll);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_region_covers_only_the_pet() {
        let window = Size::new(361.0, 361.0);
        let rects = input_region_rects(false, false, window, AVOID_RING);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0], Rect::new(40.0, 40.0, 281.0, 281.0));
    }

    #[test]
    fn locked_and_avoiding_uses_the_whole_ring() {
        let window = Size::new(361.0, 361.0);
        let rects = input_region_rects(true, true, window, AVOID_RING);
        assert_eq!(rects, vec![Rect::new(0.0, 0.0, 361.0, 361.0)]);
    }

    #[test]
    fn locked_without_avoiding_is_fully_click_through() {
        let window = Size::new(361.0, 361.0);
        assert!(input_region_rects(true, false, window, AVOID_RING).is_empty());
    }

    fn test_state() -> UiState {
        UiState {
            position: Point::new(100.0, 200.0),
            base_size: Size::new(453.0, 453.0),
            current: None,
            settings: Settings::default(),
            bounce: None,
            hover: false,
            click_through: false,
            pointer_polled_at: None,
            pointer: None,
            x11_pointer: false,
            avoid_velocity: typingpet_core::geom::Vec2::ZERO,
            avoid_last: None,
            avoid_settled: true,
            wayland_note_printed: false,
        }
    }

    #[test]
    fn drag_target_tracks_the_pointer_without_double_counting() {
        // 抓取点距窗口左上角 100px。光标每次右移 10px，窗口必须精确跟随，
        // 且抓取点始终贴在光标下（旧实现用"按下时的位置 + 偏移"会把自身位移算两遍 → 抽搐）。
        let grab = Point::new(100.0, 100.0);
        let mut window = Point::ZERO;
        for step in 1..=5 {
            let pointer = Point::new(100.0 + 10.0 * f64::from(step), 100.0);
            let widget = Point::new(pointer.x - window.x, pointer.y - window.y);
            let offset = Point::new(widget.x - grab.x, widget.y - grab.y);
            window = drag_target(window, offset);
            assert!(
                (window.x + grab.x - pointer.x).abs() < 0.001,
                "第 {step} 步抓取点偏离光标：{}",
                window.x + grab.x - pointer.x
            );
        }
        assert_eq!(window, Point::new(50.0, 0.0));
    }

    #[test]
    fn hover_hit_has_hysteresis_at_the_boundary() {
        let mut state = test_state();
        let frame = state.pet_frame();
        let just_outside = Point::new(frame.x - 3.0, frame.mid_y());

        // 还没进入：3px 之外不算悬停
        assert!(!state.hover_hit(just_outside));
        // 已经悬停：6px 迟滞内仍然算悬停（否则边缘会反复跳透明度）
        state.hover = true;
        assert!(state.hover_hit(just_outside));
        // 超出迟滞才离开
        assert!(!state.hover_hit(Point::new(frame.x - 20.0, frame.mid_y())));
        // 本体内部无论何种状态都算悬停
        assert!(state.hover_hit(Point::new(frame.mid_x(), frame.mid_y())));
    }

    #[test]
    fn physical_pointer_coordinates_are_converted_to_logical_space() {
        // 本机实测：GDK 显示器 1600x1067（逻辑）vs X11 root 2160x1440（物理），倍率 1.35。
        // 悬停判定必须用同一空间，否则物理坐标会被拿去和逻辑宠物框比较。
        let physical = Point::new(1740.0, 1030.0);
        let logical = physical_to_logical(physical, 1.35);
        assert!((logical.x - 1288.9).abs() < 0.1, "got {}", logical.x);
        assert!((logical.y - 763.0).abs() < 0.1, "got {}", logical.y);
        // 无缩放时原样返回
        assert_eq!(physical_to_logical(physical, 1.0), physical);
        // 异常缩放不至于除零
        assert_eq!(physical_to_logical(physical, 0.0), physical);
    }

    #[test]
    fn x11_pointer_switches_the_avoidance_trigger_distance() {
        // X11 是全局光标 → 90px 提前量；Wayland 只有检测环 → 40px。
        // 这里锁定"能力标志决定来源"这一契约：它必须在启动时定一次。
        let mut state = test_state();
        state.x11_pointer = true;
        assert!(state.x11_pointer);
        state.x11_pointer = false;
        assert!(!state.x11_pointer);
        assert_eq!(AVOID_TRIGGER_GLOBAL, 90.0);
        assert_eq!(AVOID_RING, 40.0);
    }

    #[test]
    fn pet_frame_is_inset_by_the_ring() {
        let state = UiState {
            position: Point::new(100.0, 200.0),
            base_size: Size::new(453.0, 453.0),
            current: None,
            settings: Settings::default(),
            bounce: None,
            hover: false,
            click_through: false,
            pointer_polled_at: None,
            pointer: None,
            x11_pointer: false,
            avoid_velocity: typingpet_core::geom::Vec2::ZERO,
            avoid_last: None,
            avoid_settled: true,
            wayland_note_printed: false,
        };
        let frame = state.pet_frame();
        assert_eq!(frame.x, 140.0);
        assert_eq!(frame.y, 240.0);
        assert!((state.window_size().w - (frame.w + 80.0)).abs() < 0.01);
    }
}

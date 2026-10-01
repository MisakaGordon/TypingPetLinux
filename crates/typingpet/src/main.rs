//! TypingPet Linux 原型入口。
//!
//! 默认启动 GTK4 宠物窗口；`--headless` 时不开窗，只在终端打印状态机动作，
//! 便于在容器/CI 里做全链路验证。

mod assets;
#[cfg(feature = "gui")]
mod autostart;
#[cfg(feature = "gui")]
mod gui;
#[cfg(feature = "gui")]
mod settings_window;
#[cfg(feature = "gui")]
mod tray;

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use typingpet_core::config::{default_config_path, default_data_directory, Settings};
use typingpet_core::keys::{KeyModifiers, KeyReactionStore, KeyStroke};
use typingpet_core::library::ImageLibrary;
use typingpet_core::picker::SystemRng;
use typingpet_core::state::{Action, PetState};
use typingpet_input::InputHandle;

#[derive(Debug, Clone)]
pub struct Options {
    pub headless: bool,
    pub mock_keystrokes: Option<usize>,
    pub config_path: PathBuf,
    pub data_directory: PathBuf,
    pub scale_override: Option<f64>,
    pub click_through: bool,
    pub no_tray: bool,
    pub no_layer_shell: bool,
    pub layer: String,
    pub dump_png: Option<PathBuf>,
    pub position: Option<(f64, f64)>,
    pub reset_position: bool,
    pub open_settings: Option<bool>,
    /// 拿不到键盘设备时直接失败退出（脚本/CI 用；默认是降级运行并自动重试）
    pub require_input: bool,
    /// 只做键输入自检然后退出
    pub check_input: bool,
    pub settings_tab: u32,
    pub avoid_pointer: bool,
    pub simulate_pointer: Option<(f64, f64)>,
    pub print_events: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            headless: false,
            mock_keystrokes: None,
            config_path: default_config_path(),
            data_directory: default_data_directory(),
            scale_override: None,
            click_through: false,
            no_tray: false,
            no_layer_shell: false,
            layer: "top".to_string(),
            dump_png: None,
            position: None,
            reset_position: false,
            open_settings: None,
            require_input: false,
            check_input: false,
            settings_tab: 0,
            avoid_pointer: false,
            simulate_pointer: None,
            print_events: false,
        }
    }
}

impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut options = Options::default();
        let mut args = args.into_iter().peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--headless" => options.headless = true,
                "--mock" => options.mock_keystrokes = Some(12),
                "--click-through" => options.click_through = true,
                "--no-tray" => options.no_tray = true,
                "--no-layer-shell" => options.no_layer_shell = true,
                "--layer" => {
                    options.layer = args.next().context("--layer needs a value")?;
                }
                "--reset-position" => options.reset_position = true,
                "--settings" => options.open_settings = Some(true),
                "--require-input" => options.require_input = true,
                "--check-input" => options.check_input = true,
                "--avoid-pointer" => options.avoid_pointer = true,
                "--simulate-pointer" => {
                    let value = args.next().context("--simulate-pointer needs X,Y")?;
                    let (x, y) = value.split_once(',').context("--simulate-pointer expects X,Y")?;
                    options.simulate_pointer = Some((
                        x.trim().parse().context("X must be a number")?,
                        y.trim().parse().context("Y must be a number")?,
                    ));
                }
                "--settings-tab" => {
                    let value = args.next().context("--settings-tab needs a value")?;
                    options.settings_tab = match value.as_str() {
                        "general" | "0" => 0,
                        "gallery" | "1" => 1,
                        "keys" | "rules" | "2" => 2,
                        other => anyhow::bail!("unknown settings tab: {other}"),
                    };
                }
                "--position" => {
                    let value = args.next().context("--position needs X,Y")?;
                    let (x, y) = value
                        .split_once(',')
                        .context("--position expects X,Y")?;
                    options.position = Some((
                        x.trim().parse().context("--position X must be a number")?,
                        y.trim().parse().context("--position Y must be a number")?,
                    ));
                }
                "--dump-png" => {
                    options.dump_png =
                        Some(PathBuf::from(args.next().context("--dump-png needs a path")?));
                }
                "--print-events" => options.print_events = true,
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                "--scale" => {
                    let value = args.next().context("--scale needs a value")?;
                    options.scale_override = Some(value.parse().context("--scale must be a number")?);
                }
                "--config" => {
                    options.config_path = PathBuf::from(args.next().context("--config needs a path")?);
                }
                "--data" => {
                    options.data_directory =
                        PathBuf::from(args.next().context("--data needs a path")?);
                }
                other if other.starts_with("--mock=") => {
                    let value = other.trim_start_matches("--mock=");
                    options.mock_keystrokes = Some(value.parse().context("--mock=N")?);
                }
                other => anyhow::bail!("unknown argument: {other} (try --help)"),
            }
        }
        Ok(options)
    }
}

fn print_help() {
    println!(
        "typingpet {}\n\
         \n\
         用法: typingpet [选项]\n\
         \n\
           --headless          不开窗，在终端打印状态机动作（容器/CI 验证用）\n\
           --mock[=N]          使用脚本化按键输入源（默认 12 次），无需键盘权限\n\
           --scale F           覆盖宠物缩放 (0.35 - 1.25)\n\
           --click-through     启动即锁定位置（鼠标穿透）\n\
           --no-tray           不注册托盘图标\n\
           --no-layer-shell    强制使用普通无边框窗口（不启用 layer-shell）\n\
           --layer L           layer-shell 层级：top(默认) | overlay | bottom\n\
           --dump-png PATH     启动 2 秒后把窗口内容渲染成 PNG（不依赖合成器，自检用）\n\
           --position X,Y      强制宠物左上角坐标（屏幕逻辑像素）\n\
           --reset-position    忽略已保存的位置，回到主显示器右下角\n\
           --settings          启动时直接打开设置窗口\n\
           --check-input       只做键输入自检（设备/权限），然后退出\n\
           --require-input     拿不到键盘设备就报错退出（默认改为降级运行并自动重试）\n\
           --settings-tab T    设置窗口初始标签页：general | gallery | keys\n\
           --avoid-pointer     启动即开启「位置锁定中躲避光标」\n\
           --simulate-pointer X,Y  把光标假装在 (X,Y)（调试躲避逻辑用）\n\
           --print-events      打印每一次检测到的按键\n\
           --config PATH       配置文件路径（默认 $XDG_CONFIG_HOME/typingpet/config.json）\n\
           --data DIR          数据目录（默认 $XDG_DATA_HOME/typingpet）\n\
           -h, --help          显示本帮助\n",
        env!("CARGO_PKG_VERSION")
    );
}

/// 命令行覆盖了哪些设置。这些**只影响本次会话**，绝不写进配置文件。
#[derive(Debug, Clone, Default)]
pub struct AppliedOverrides {
    pub scale: Option<f64>,
    pub position_locked: Option<bool>,
    pub avoids_pointer_when_locked: Option<bool>,
    pub pet_position: Option<(f64, f64)>,
}

/// 一次运行所需的全部状态，GUI 与 headless 共用。
pub struct Runtime {
    pub options: Options,
    pub settings: Settings,
    /// 配置文件里的原始值（CLI 覆盖之前），用于"覆盖项不落盘"
    pub baseline_settings: Settings,
    pub overrides: AppliedOverrides,
    pub library: ImageLibrary,
    pub rules: KeyReactionStore,
    pub state: PetState,
    pub input: InputHandle,
    pub started: Instant,
    pub rng: SystemRng,
}

impl Runtime {
    pub fn bootstrap(options: Options) -> Result<Self> {
        let baseline_settings = Settings::load(&options.config_path);
        let resources = options.data_directory.join("BuiltIn");
        assets::ensure_builtin_assets(&resources).context("release built-in assets")?;

        let library = ImageLibrary::new(options.data_directory.join("Images"), &resources);
        let rules = KeyReactionStore::new(options.data_directory.join("KeyReactions"));

        let mut settings = baseline_settings.clone();
        let mut overrides = AppliedOverrides::default();
        if let Some(scale) = options.scale_override {
            settings.scale = scale;
            overrides.scale = Some(scale);
        }
        if options.click_through {
            settings.position_locked = true;
            overrides.position_locked = Some(true);
        }
        if options.avoid_pointer {
            settings.avoids_pointer_when_locked = true;
            overrides.avoids_pointer_when_locked = Some(true);
            settings.position_locked = true;
            overrides.position_locked.get_or_insert(true);
        }
        if let Some((x, y)) = options.position {
            settings.pet_x = x;
            settings.pet_y = y;
            overrides.pet_position = Some((x, y));
        }
        let settings = settings.clamped();

        let state = PetState::new(
            library.idle_url(),
            library.reaction_urls(),
            settings.shake_amplitude(),
        );

        let input = match options.mock_keystrokes {
            Some(count) => typingpet_input::open_mock(mock_script(count), Duration::from_millis(120)),
            None if options.require_input => typingpet_input::open_evdev()
                .context("global keyboard capture unavailable（--require-input 指定了必须可用）")?,
            None => typingpet_input::open_evdev_or_retry(),
        };
        if !input.is_connected() {
            // 不再直接退出：桌宠照常显示，只是暂时不响应按键，后台每 2 秒重试
            println!();
            println!("⚠️  键输入暂时不可用：{}", input.description());
            println!("    桌宠会继续显示（悬停/拖动/设置/托盘都正常），程序每 2 秒自动重试。");
            print_input_help();
            println!();
        }

        Ok(Self {
            options,
            settings,
            baseline_settings,
            overrides,
            library,
            rules,
            state,
            input,
            started: Instant::now(),
            rng: SystemRng::new(),
        })
    }

    pub fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    pub fn description(&self) -> String {
        format!(
            "session={} input={} reactions={} idle={}",
            typingpet_input::session_kind(),
            self.input.description(),
            self.library.reaction_urls().len(),
            self.library
                .idle_url()
                .map(|path| path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default())
                .unwrap_or_else(|| "<none>".to_string())
        )
    }
}

fn mock_script(count: usize) -> Vec<KeyStroke> {
    // A / S / D / F 交替，外加一次 Ctrl+K 组合，用于演示"精确规则优先于随机反应"。
    const KEYS: [u32; 4] = [30, 31, 32, 33];
    (0..count)
        .map(|index| {
            if index % 7 == 6 {
                KeyStroke::new(37, KeyModifiers::empty().with(KeyModifiers::CONTROL))
            } else {
                KeyStroke::new(KEYS[index % KEYS.len()], KeyModifiers::NONE)
            }
        })
        .collect()
}

/// 键输入不可用时的可执行指引（尽量给出能直接复制的命令）。
pub fn print_input_help() {
    println!("    排查与修复：");
    println!("      · 先看设备与权限：  typingpet --check-input");
    println!("      · 装 udev 规则（只授权当前登录用户，登出失效）：");
    println!("          sudo install -m644 <包里的>/share/typingpet/60-typingpet-input.rules \\");
    println!("               /etc/udev/rules.d/60-typingpet-input.rules");
    println!("          sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=input");
    println!("      · 或加入 input 组（安全性更低，需重新登录）：");
    println!("          sudo usermod -aG input \"$USER\"");
    println!("      · 如果你在容器/沙箱里运行（例如 DSH 会话），里面通常没有 /dev/input，");
    println!("        请在宿主桌面会话里运行本程序。");
}

/// `--check-input`：打印设备、权限与结论。
pub fn check_input() -> Result<()> {
    println!("== TypingPet 键输入自检 ==");
    println!("会话类型 : {}", typingpet_input::session_kind());
    println!("DISPLAY  : {}", std::env::var("DISPLAY").unwrap_or_else(|_| "<未设置>".into()));
    println!();
    println!("/dev/input 下的设备：");
    let devices = typingpet_input::evdev_source::describe_devices();
    if devices.is_empty() {
        println!("  （看不到任何输入设备：/dev/input 不存在或为空）");
    } else {
        for line in &devices {
            println!("  {line}");
        }
    }
    println!();
    match typingpet_input::open_evdev() {
        Ok(handle) => {
            println!("✅ 键输入可用：{}", handle.description());
            println!("   现在可以启动桌宠：typingpet");
            Ok(())
        }
        Err(error) => {
            println!("❌ 键输入不可用：{error}");
            println!();
            print_input_help();
            std::process::exit(2);
        }
    }
}

fn main() -> Result<()> {
    let options = Options::parse(std::env::args().skip(1))?;
    if options.check_input {
        return check_input();
    }

    #[cfg(feature = "gui")]
    if !options.headless {
        return gui::run(options);
    }

    run_headless(options)
}

/// 无 GUI 的全链路验证：输入源 → 状态机 → 动作。
fn run_headless(options: Options) -> Result<()> {
    let mut runtime = Runtime::bootstrap(options)?;
    println!("== TypingPet (headless) ==");
    println!("{}", runtime.description());
    println!("config: {}", runtime.options.config_path.display());
    println!(
        "mode:   Wayland 下躲避光标为局部检测，X11 下为完整检测（session={}）",
        typingpet_input::session_kind()
    );
    println!();

    let mut rng = std::mem::replace(&mut runtime.rng, SystemRng::from_seed(1));
    for action in runtime.state.initial_actions() {
        print_action(&action, "startup");
    }

    let deadline = runtime
        .options
        .mock_keystrokes
        .map(|_| Instant::now() + Duration::from_secs(4));
    let mut idle_printed = false;

    loop {
        if let Some(deadline) = deadline {
            if Instant::now() >= deadline {
                break;
            }
        }
        if let Some(event) = runtime.input.recv_timeout(Duration::from_millis(50)) {
            if runtime.options.print_events {
                println!("  key: {}", event.stroke.display_name());
            }
            idle_printed = false;
            let actions = runtime
                .state
                .on_key(Some(event.stroke), &runtime.rules, &mut rng, runtime.now());
            for action in actions {
                print_action(&action, "key");
            }
        }

        let actions = runtime.state.tick(runtime.now());
        for action in actions {
            print_action(&action, "idle-timeout");
            idle_printed = true;
        }

        if deadline.is_none() && idle_printed {
            // 真实设备模式：持续运行
        }
        if deadline.is_some() && runtime.state.is_showing_idle() && idle_printed {
            continue;
        }
    }

    if let Some(screen) = typingpet_input::screen_size() {
        println!("screen: {}x{}", screen.width, screen.height);
    }
    match typingpet_input::pointer_position() {
        Some(pointer) => println!("pointer: x={} y={} (X11)", pointer.x, pointer.y),
        None => println!("pointer: unavailable (Wayland: 只能做局部躲避)"),
    }
    println!("done: {} action(s) exercised", runtime.state.reaction_count());
    Ok(())
}

fn print_action(action: &Action, source: &str) {
    match action {
        Action::ShowImage(path) => println!(
            "[{source}] show {}",
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string())
        ),
        Action::Bounce(amplitude) => println!("[{source}] bounce amplitude={amplitude}"),
    }
}

/// 供 GUI 层复用：把运行时放进 `Arc`，避免 GUI 回调所有权纠缠。
pub type SharedRuntime = Arc<std::sync::Mutex<Runtime>>;

pub fn shared(runtime: Runtime) -> SharedRuntime {
    Arc::new(std::sync::Mutex::new(runtime))
}

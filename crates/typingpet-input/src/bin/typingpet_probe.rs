//! 真机验证工具：列出输入设备并实时打印全局按键。
//!
//! 用法：
//!   cargo run -p typingpet-input --bin typingpet-probe            # 持续监听
//!   cargo run -p typingpet-input --bin typingpet-probe -- 10      # 监听 10 秒
//!   cargo run -p typingpet-input --bin typingpet-probe -- --list  # 只列设备

use std::time::{Duration, Instant};
use typingpet_input::evdev_source;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    println!("== TypingPet input probe ==");
    println!("session: {}", typingpet_input::session_kind());
    println!("devices:");
    let devices = describe_or_empty();
    if devices.is_empty() {
        println!("  (no /dev/input devices visible; in a container this is expected)");
    } else {
        for line in &devices {
            println!("  {line}");
        }
    }

    if args.iter().any(|arg| arg == "--list") {
        return;
    }

    let seconds: Option<u64> = args.iter().find_map(|arg| arg.parse().ok());
    match typingpet_input::open_evdev() {
        Ok(handle) => {
            println!("listening ({})", handle.description());
            if let Some(seconds) = seconds {
                println!("for {seconds}s ...");
            } else {
                println!("press Ctrl+C to stop");
            }
            let deadline = seconds.map(|seconds| Instant::now() + Duration::from_secs(seconds));
            let mut count = 0usize;
            loop {
                if let Some(deadline) = deadline {
                    if Instant::now() >= deadline {
                        break;
                    }
                }
                if let Some(event) = handle.recv_timeout(Duration::from_millis(200)) {
                    count += 1;
                    println!(
                        "[{count:>3}] code={:<4} {:<20} (device #{})",
                        event.stroke.code,
                        event.stroke.display_name(),
                        event.device_index
                    );
                }
            }
            println!("total: {count} keystroke(s)");
        }
        Err(error) => {
            eprintln!("cannot listen: {error}");
            eprintln!();
            eprintln!("修复方式（任选其一，需重新登录）：");
            eprintln!("  sudo install -m644 packaging/60-typingpet-input.rules /etc/udev/rules.d/");
            eprintln!("  sudo udevadm control --reload && sudo udevadm trigger");
            eprintln!("  # 或：sudo usermod -aG input \"$USER\"");
            std::process::exit(2);
        }
    }

    if let Some(screen) = typingpet_input::screen_size() {
        println!("screen: {}x{}", screen.width, screen.height);
    }
    match typingpet_input::pointer_position() {
        Some(pointer) => println!("pointer: x={} y={}", pointer.x, pointer.y),
        None => println!("pointer: unavailable (Wayland session)"),
    }
}

fn describe_or_empty() -> Vec<String> {
    std::panic::catch_unwind(evdev_source::describe_devices).unwrap_or_default()
}

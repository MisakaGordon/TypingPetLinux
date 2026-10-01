# TypingPet for Linux

A desktop pet that reacts to key presses — a native Linux version implemented in **Rust + GTK4**, developed and verified on KDE Plasma 6 / Wayland.

> This project originates from the macOS version [TypingPet](../../) (author: MisakaGordon, implemented in Swift + AppKit/SwiftUI).
> **The original macOS version is preserved in the `macos-swift` branch**; this branch (`main`) contains only the Linux version.

When you press keys the pet switches to a random reaction image and bounces; after 0.75 seconds without input it returns to the idle image. You can assign specific images to certain keys or key combinations.

## Features

- **Trigger on any key**: switch to a random reaction image (won't repeat the same image consecutively) + bounce animation scaled by press intensity
- **Key-specific reactions**: exact-match rules take precedence over random reactions; supports `Ctrl/Alt/Shift/Super` combinations
- **Animated images**: GIF / animated WebP will actually animate (supported for both idle and reaction images)
- **Idle restore**: returns to the idle image after 0.75s with no input
- **Opacity**: separate persistent / hover opacity values are adjustable and take effect immediately
- **Window behaviors**: always-on-top, position lock (click-through), drag to move, mouse-wheel scaling (0.35–1.25)
- **Avoid pointer**: the pet moves away when the pointer gets close (X11 uses a 90px pre-check; Wayland uses a 40px detection ring and local detection)
- **Autostart**: writes `~/.config/autostart/typingpet.desktop`
- **Tray menu**: settings / show-hide / always on top / click-through / reset position / size presets / status / quit
- **Settings window**: three pages — General · Gallery · Key reactions
- **Gallery**: import image folders, switch / rename / delete galleries
- **Privacy**: only reads keycodes and press states, does not read characters, does not write logs to disk, and does not access the network

## Build and run

Requires Rust 1.75+ and GTK4 development packages:

```bash
# Fedora
sudo dnf install -y gtk4-devel gtk4-layer-shell-devel
# Ubuntu / Debian (gtk4-layer-shell may need building from source on some distributions)
sudo apt install -y libgtk-4-dev libgtk4-layer-shell-dev

git clone https://github.com/MisakaGordon/TypingPetLinux.git
cd TypingPetLinux
cargo pet                  # = cargo run --release -p typingpet
```

> ⚠️ `cargo run -p typingpet-input` runs the **keyboard input probe** (prints keys only, no window).
> The pet window is in the `typingpet` package. The repository defines shortcuts: `cargo pet` (main app) / `cargo probe` (probe).

### Keyboard input permissions (Wayland has no equivalent to macOS `CGEventTap`)

To receive "any key" on both X11 and Wayland you must read `/dev/input/event*` directly. Install a udev rule to grant access
(to the current logged-in user via ACL; the permission is revoked on logout):

```bash
sudo install -m644 packaging/60-typingpet-input.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=input
cargo probe -- 10        # first confirm you can read key events
```

Alternative (less secure): add your user to the `input` group with `sudo usermod -aG input "$USER"` and re-login.

### Packaging and distribution (tarball)

```bash
sh packaging/build-tarball.sh          # artifact in dist/, includes sha256
tar xf dist/typingpet-*.tar.gz && cd typingpet-*/
sh install.sh --check                  # compatibility check first, no system changes
sh install.sh                          # install to ~/.local
sudo sh install.sh --system            # install to /usr/local (can also install udev rule)
sh install.sh --uninstall              # uninstall
```

The package ships `libgtk4-layer-shell.so.0` (56KB) — many distributions do not include this package and the program will not start without it;
the binary looks for it with `$ORIGIN/../lib`. If the system already has the library, the installer will skip the bundled copy.

Compatibility lower bounds (`install.sh --check` will check and provide distribution-specific install commands):

| Item | Requirement | Notes |
|---|---:|---|
| glibc | ≥ 2.39 | Covers Fedora 40+ / Ubuntu 24.04+ / Debian 13+ / Arch / openSUSE TW |
| GTK4 | ≥ 4.12 | Needs runtime libraries (install `gtk4` on Fedora, `libgtk-4-1` on Debian/Ubuntu) |

> Debian 12 (glibc 2.36 / GTK 4.8), Ubuntu 22.04 (2.35 / 4.6), RHEL 9 (2.34 / 4.6)
> are currently not supported: the glibc lower bound comes from zbus's process-creation path (tested — see `docs/RUST_PROTOTYPE.md`).
> To support those older baselines you would need to build inside an older base container; the script will give explicit hints instead of failing with linker errors.

### Common options

```
--headless          No window; print state-machine actions to terminal (CI/container verification)
--mock[=N]          Scripted key input source; no keyboard permissions required
--settings          Open settings window on startup (--settings-tab general|gallery|keys)
--scale F           Scale override for this session    --position X,Y   Initial coordinates for this session
--click-through     Start with position locked (click-through)
--avoid-pointer     Start with avoid-pointer enabled
--layer L           layer-shell layer: top (default) | overlay | bottom
--no-layer-shell    Force a normal borderless window
--dump-png PATH     Render window contents to PNG (self-check)
```

> `--click-through` / `--avoid-pointer` / `--scale` only affect the current session and are not written to the configuration file;
> only values you change explicitly in the settings window or tray will be saved.

## Wayland compatibility

Wayland's security model does not provide the macOS-style global input/window control APIs. This project fills the gaps using platform-allowed mechanisms:

| Capability | Approach |
|---|---|
| Global keyboard (any key) | evdev direct read of `/dev/input/event*` + udev `uaccess` (equivalent for X11/Wayland) |
| Always-on-top | `Layer::Top` from `gtk4-layer-shell` |
| Window positioning | layer-shell `anchor(Top|Left)` + `margin` |
| Mouse click-through | set an empty input region with `wl_surface.set_input_region` |
| Hover opacity | UI events + pet rectangle hit-test |
| Avoid pointer | 40px detection ring: window is 40px larger on each side than the image; when locked, put the ring into the input region and use pointer events inside the ring to trigger escape movement |
| Tray / settings window / file chooser | StatusNotifierItem + normal xdg-toplevel + xdg-desktop-portal |

Trade-off: Wayland's avoidance is "move away when close" (40px) and while "locked + avoiding" that ring will capture mouse clicks —
turn off avoidance to restore full click-through. The settings window documents this behavior.

Platform limitation: GNOME (Mutter) does not support `wlr-layer-shell`; on GNOME Wayland the app cannot be made always-on-top/positioned,
and will fall back to a normal borderless window with a prompt. X11 and KDE Wayland are not affected.

## Code layout

```
crates/
├─ typingpet-core/     Pure logic: geometry / inertial physics / avoidance, random image selection, gallery & directory rules, key model, configuration, state machine
├─ typingpet-input/    evdev global keys (+ mock), X11 pointer queries, typingpet-probe probe
└─ typingpet/          GTK4 pet window, settings window, tray, headless mode
packaging/             udev rules, .desktop
```

Design note: core logic has zero GUI dependency; the input layer and configuration storage are abstracted and replaceable,
so all logic can be tested in headless environments (CI/containers). `cargo test --workspace` runs 43 tests.

## License

- Source code: [MIT](LICENSE)
- Built-in keyboard-cat assets: [CC0 1.0](ASSET_LICENSE.md)

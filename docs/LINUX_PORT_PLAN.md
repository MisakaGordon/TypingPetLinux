# TypingPet macOS → Linux 原生移植计划

> 结论先行：这个项目的**业务逻辑几乎全是可移植的纯逻辑**（几何/物理/挑图/目录规则/规则存储），
> 真正的移植成本集中在 4 个 Linux 桌面平台能力上：**全局键盘监听、无边框置顶+点击穿透、全局光标位置、托盘**。
> 建议做法：先把纯逻辑抽成不依赖 AppKit 的 Swift 核心库（今天就能在 Linux 上跑 `swift test`），
> 再为 GTK4 写新的平台层，最后按 X11 / KDE-Wayland / GNOME-Wayland 三级降级矩阵验收。

---

## 1. 目标与非目标

> **2026-09-26 更新**：技术栈已改为 **Rust + GTK4（S3）**，不再保留 macOS 版。
> 已按本文档的 P0–P3 落地并在真机 KDE Wayland 上验证通过，见 `docs/RUST_PROTOTYPE.md`。
> 下文 §5–§7、§10 中的 Swift 方案已作废，保留仅为记录决策过程；§1–§4 的平台分析仍然有效。

**目标**
- 在 Linux（首选 Fedora KDE Plasma，Wayland 会话）上得到行为与 macOS 版一致的宠物窗口。
- 功能对齐：任意按键触发随机反应图、按键/组合专属图、0.75s 回 idle、拖拽+惯性、悬停控制（×/缩放手柄）、
  常驻/悬停透明度、弹跳强度、位置锁定（点击穿透）、锁定态躲避光标、图库多套图、设置窗口三标签页、开机自启。
- macOS 版本继续可编译、可发布（同一仓库，双平台）。

**非目标（首版不做）**
- 不做 macOS 与 Linux 之间的设置迁移（键码空间不同，见 §4.1）。
- 不追求 GNOME/Wayland 下的完全功能对等（见 §4.3 的能力限制），但要有明确降级行为。
- 不做插件系统、网络功能、多用户共享配置。

---

## 2. 代码现状（本次实测）

### 2.1 结构与规模

| 文件 | 行数 | 职责 | 平台耦合 |
|---|---:|---|---|
| `Sources/TypingPet/main.swift` | 1417 | 宠物面板、拖拽/缩放/惯性/躲避物理循环、全局键监听、状态栏菜单、AppDelegate | **全部 AppKit** |
| `Sources/TypingPet/SettingsWindow.swift` | 634 | 设置模型 + SwiftUI 三标签页 + 按键捕获 Sheet | **AppKit + SwiftUI** |
| `Sources/TypingPet/PetImageLibrary.swift` | 309 | 图库集/目录导入规则/内置资源解析 | Foundation + 少量 AppKit |
| `Sources/TypingPet/PetWindowGeometry.swift` | 188 | 透明度、躲避目标点、拖拽速度、惯性推进、缩放几何 | **纯逻辑（仅缺 `CGVector`）** |
| `Sources/TypingPet/KeyReactions.swift` | 136 | 按键模型/修饰键/规则存储/键名表 | Foundation + AppKit 仅用于校验图片 |
| `Sources/TypingPet/ImagePicker.swift` | 19 | 随机挑图（排除当前图） | **纯逻辑** |
| `Tests/TypingPetTests/*` | 386 | 4 个测试文件 | XCTest，1 处 `import CoreGraphics` |

### 2.2 macOS API 依赖密度（脚本统计的去重符号种类 / 出现次数）

```
main.swift                  43 / 190
SettingsWindow.swift        20 /  52
PetWindowGeometry.swift      5 /  53   (CGFloat/CGPoint/CGRect/CGSize/CGVector)
KeyReactions.swift           2 /   2
PetImageLibrary.swift        1 /   1
ImagePicker.swift            0 /   0
```
调用量 Top：`NSEvent`(24)、`NSMenuItem`(14)、`NSImage`(14)、`SMAppService`(9)、
`CGPreflightListenEventAccess`(5)、`NSTrackingArea`(4)、`NSOpenPanel`(4)、`NSImageView`(4)、`NSStatusItem`(2)、`NSCursor`(3)…

### 2.3 Linux 可移植性实测（本机 Swift 6.4 / x86_64 Fedora 容器）

| 文件 | 结果 |
|---|---|
| `ImagePicker.swift` | ✅ `swiftc -typecheck` 直接通过 |
| `PetWindowGeometry.swift` | ⚠️ 仅 `CGVector` / `.zero` 缺失（Linux Foundation 有 `CGFloat/CGPoint/CGRect/CGSize`，没有 `CGVector`），加 6 行 shim 即可 |
| `KeyReactions.swift` | ⛔ `no such module 'AppKit'`（`NSImage` 校验 + `CGEventFlags`） |
| `PetImageLibrary.swift` | ⛔ `no such module 'AppKit'`（`NSImage` + `Bundle.main` + `UserDefaults`） |
| 整包 `swift build` | ⛔ 预期失败（`no such module 'AppKit'`） |

> 结论：**约 600 行纯逻辑可平移复用**，`main.swift` + `SettingsWindow.swift`（2051 行）必须重写为 GTK4 平台层。
> 现在就能把纯逻辑拆出来在 Linux 上跑单测，且这一步**与后面选哪种 UI 技术栈无关**。

### 2.4 运行环境实测

| 项 | 结果 |
|---|---|
| 会话 | Fedora 44 KDE Plasma，`XDG_SESSION_TYPE=wayland`，`XDG_CURRENT_DESKTOP=KDE` |
| Swift | 6.4 `x86_64-unknown-linux-gnu`（swiftly） |
| GTK4 运行时 | ✅ `libgtk-4.so.1`、`libgdk_pixbuf-2.0.so.0`、PyGObject `Gtk 4.0` 可用 |
| libadwaita / layer-shell / 托盘 | ✅ `Adw-1`、`Gtk4LayerShell-1.0`、`AppIndicator3-0.1`、`AyatanaAppIndicator3-0.1` typelib 齐备 |
| GTK4 hello-window 冒烟 | ✅ 在容器内经 `/run/user/1000/wayland-0` 成功 `present()`（`GdkWaylandDisplay`） |
| 开发头文件 | ⛔ `gtk4-devel` / `gobject-introspection-devel` / `libevdev-devel` / `libX11-devel` 均未安装 |
| root / sudo | ⛔ 容器内无 root（`sudo` 被 `no new privileges` 拒绝） |
| `/dev/input` | ⛔ 容器内不存在（**evdev 相关功能必须到宿主桌面验证**） |

---

## 3. 平台能力映射表

| macOS 能力 | 代码位置 | Linux 替代 | 风险 |
|---|---|---|---|
| `CGEventTap` 全局按键（listen-only） | `GlobalKeyMonitor` | evdev(`/dev/input/event*`) 为主；X11 XRecord/XI2 为备 | **高** |
| `NSWindow` 无边框 + `ignoresMouseEvents` 点击穿透 | `PetPanel`、`PetController.isPositionLocked` | X11: `_NET_WM_STATE_ABOVE`+input shape；Wayland: `gtk4-layer-shell` + `can_target(false)` | **高** |
| `NSEvent.mouseLocation` 全局光标 | `updatePetOpacity`、`updatePointerAvoidance` | Wayland 无此 API；X11 `XQueryPointer`；KDE 可经 KWin 脚本 | **高** |
| `NSStatusItem` + `NSMenu` 状态栏 | `configureStatusItem()` | StatusNotifierItem（libayatana-appindicator 或自实现 GDBus SNI） | 中 |
| `NSImage` / `NSImageView.animates` | `PetImageLibrary`、`showImage` | `GdkPixbuf` + 逐帧 tick 驱动（GIF/WebP 动图） | 中 |
| `CASpringAnimation` / `CAKeyframeAnimation` | `animateImageScale`、`bounce` | `GtkWidget` tick callback 手写弹簧（可复用现有常量）；libadwaita ≥1.6 有 `AdwSpringAnimation` | 低 |
| `NSTrackingArea` 悬停 | `PetContentView` | `GtkEventControllerMotion` (`enter`/`leave`/`motion`) | 低 |
| `NSOpenPanel` 选图/选目录 | `chooseImageFiles`、`importImageFolder` | `GtkFileDialog`（走 xdg-desktop-portal） | 低 |
| `NSAlert` | `showAlert` | `GtkAlertDialog` / `AdwMessageDialog` | 低 |
| `UserDefaults` | 全局 | JSON（`~/.config/typingpet/config.json`）或 GSettings | 低 |
| `Bundle.main.url(forResource:)` | `PetImageLibrary.resolvedURL` | 可执行文件同级 `share/typingpet/` / `/app/share/…` | 低 |
| `SMAppService` 开机自启 | `toggleLaunchAtLogin` | `~/.config/autostart/typingpet.desktop` | 低 |
| `NSCursor.frameResize` | `PetResizeHandleView` | `GdkCursor("nwse-resize")` / `"se-resize"` | 低 |
| 辅助功能标签 | `setAccessibilityLabel` | `gtk_accessible_update_property`（AT-SPI 自动生效） | 低 |
| 输入监控权限面板 | `openInputMonitoringSettings` | 文档化 udev 规则 + `usermod -aG input`；可选 polkit 一键安装器 | 中 |
| 键码 + 显示名（macOS 虚拟键码表） | `KeyCodeNames` | evdev code(`KEY_*`) + `libxkbcommon` 解析布局显示名 | 中 |

---

## 4. 关键难点与选定方案

### 4.1 全局键盘监听（决定项目成败的第一难点）

Wayland 的设计原则就是"禁止键盘记录器"，因此**没有** macOS `CGEventTap` 的等价物。可选路径：

| 方案 | 覆盖范围 | 权限成本 | 结论 |
|---|---|---|---|
| **A. evdev 直读** `/dev/input/event*`（libevdev + udev 枚举/热插拔） | X11 与 Wayland **全量按键**，与前台是否为原生 Wayland 无关 | 需要一个 udev `TAG+="uaccess"` 规则或加入 `input` 组 | ✅ **主方案** |
| B. X11 XRecord / XInput2 raw key | KDE Wayland 默认策略下**只放行"带修饰键"的按键**（见下），且需应用跑在 XWayland | 零权限 | ⚠️ 降级备选（组合键规则可用，普通字母不行） |
| C. `xdg-desktop-portal` GlobalShortcuts | 只能注册用户逐个确认的**具体组合键**，拿不到"任意按键" | 每规则一次授权弹窗 | ⚠️ 只作组合键规则的可选后端 |
| D. KWin 脚本 / KGlobalAccel | 只能拿到已注册快捷键 | 零权限 | ⚠️ 同上 |

> B 的依据：KWin 自 Plasma 5.27 起提供 "Xwayland 安全/兼容" KCM，并在
> [commit a136a159](https://invent.kde.org/plasma/kwin/-/commit/a136a159f92a41cce749eb96b5f4e8c61265c1c3)
> 把默认值从 `None` 改成 `Combinations`——即默认允许 XWayland 应用读取**按住修饰键时的按键**。
> 普通字母（无修饰键）默认仍被拦截，所以"打字就触发"这一核心行为在 B 方案下不成立。

**evdev 权限模型**（按安全性递增）：
1. `usermod -aG input`：一行搞定，但用户可读全部输入设备（含密码输入）。
2. 随包安装 udev 规则（推荐默认）：
   `/etc/udev/rules.d/60-typingpet-input.rules`
   `SUBSYSTEM=="input", KERNEL=="event*", ENV{ID_INPUT_KEYBOARD}=="1", TAG+="uaccess"`
   只对当前 seat 的活动用户授予 ACL，登出即失效。
3. 特权分离（后续加固）：systemd system service 持有设备，只经 D-Bus 发"某键+修饰键"信号，UI 进程零设备权限。

**隐私对齐**：只读 `EV_KEY` 的 code 与按下/抬起状态自行推导修饰键，不读 `EV_MSC`/字符输入、不落盘、不联网，
与 README 里对 macOS 版的隐私承诺保持一致，并在首次启动提示中说明。

**键码空间变更**：macOS 虚拟键码（0–126，如 `40=K`）与 evdev（`38=K`）完全不同 →
新配置命名空间（`config.json`），Linux 版从空规则集开始；`KeyCodeNames` 改为
`libxkbcommon`：`xkb_state_key_get_utf32(state, evdevcode+8)` 取字符，
`xkb_keysym_get_name()` 兜底，非字符键（回车/F1/方向键）走静态表 + 韩文界面里的既定叫法。

### 4.2 无边框置顶 + 点击穿透

- **X11 会话**：`gtk_window_set_decorated(false)` + `_NET_WM_STATE_ABOVE`（或 `set_keep_above`），
  点击穿透用空 input shape（GTK4：根部件 `gtk_widget_set_can_target(false)`，等价于空输入区域）。
- **Wayland 会话**：xdg-toplevel **没有** keep-above/定位语义 → 用 **`gtk4-layer-shell`**
  （本机已装 `libgtk4-layer-shell.so.0` + typelib，Fedora 有包；KWin 支持 `zwlr_layer_shell_v1`，
  P3 阶段需在真机验证）：`LAYER_TOP`、`anchor` + `margin`、关闭键盘交互（`keyboard-mode: none`）、
  透明背景、空输入区域实现点击穿透（`wl_surface.set_input_region`，即 `can_target(false)`）。
- 位置需要自己持久化（layer-shell 的 margin/anchor 或 X11 的窗口坐标），沿用现有 `saveFrame` 语义改为
  "写 config.json"。
- 跨工作区/全屏覆盖语义（macOS `canJoinAllSpaces` + `fullScreenAuxiliary`）在 Wayland 无对应；
  layer-shell 天然常驻所有工作区，X11 下用 `_NET_WM_STATE_STICKY`。

### 4.3 全局光标位置（悬停透明度 + 锁定态躲避）

这是 Wayland 下**唯一无法完美对齐**的能力，必须显式降级：

| 需求 | 实现 | Wayland 可行性 |
|---|---|---|
| 悬停透明度（`frame.contains(mouse)`） | 宠物 surface 上的 `GtkEventControllerMotion` enter/leave | ✅ 可（surface 保留输入区域时） |
| 躲避光标（需要宠物**外部**的光标坐标） | X11: `XQueryPointer` 轮询；Wayland: 无 API | ⚠️ 需替代 |

**已确认方案（2026-09-26）**：
- **X11 会话**：`XQueryPointer` 轮询，功能与 macOS 版 1:1 对齐（含远距离预判）。
- **Wayland 会话**：不做 KWin 脚本桥，采用**"局部躲避"**——宠物 surface 保留一个比宠物本体大一圈
  （建议外扩 24–32px，可配置）的输入区域，光标进入该区域后才拿到 motion 事件，因此只在**靠近时**触发躲避，
  丢失远距离预判。
- **UI 必须说明差异**：设置页"位置锁定中鼠标躲避"一行在 Wayland 下附带说明文字
  （"Wayland 会话只能检测靠近宠物时的光标，躲避距离更短；需要完整行为请使用 X11 会话"），
  并给出当前会话类型（`GDK_BACKEND`/`GdkDisplay` 运行时探测结果）。
- **副作用处理**：该外扩区域会吃掉少量桌面点击 → 默认仅在"位置锁定 + 躲避开启"时启用该输入区域；
  位置锁定一关（可拖拽模式）立刻恢复为宠物本体大小，避免遮挡正常操作。

### 4.4 图像解码与动画

- 静态：`gdk-pixbuf` 覆盖 PNG/JPEG/TIFF/GIF/WebP；**HEIC/HEIF 需 `libheif-gdk-pixbuf`**，
  WebP 动态/静态建议 `libwebp-pixbuf-loader`，APNG 默认 loader 不支持（README 里的格式清单要按实际 loader 收窄）。
- 动态：GTK4 移除了 `gtk_image_set_from_animation` → 用 `GdkPixbufAnimation` + `GtkWidget` tick callback
  按 `gdk_pixbuf_animation_iter_get_delay_time()` 逐帧 `set_from_pixbuf`（顺带兼容 GIF/WebP 动图）。
- 比例自适应：沿用 `PetImageLibrary` 的"以 idle 图长边 453 归一化"逻辑，平移即可。

### 4.5 动画与弹效

- 悬停放大 `1.025` / 按下 `0.94`：`CASpringAnimation(mass 0.75, stiffness 220/360, damping 15/22)`
  → 自己写 60Hz 二阶弹簧积分（或 `AdwSpringAnimation`），复用现有常量与 `applyScale` 锚点语义。
- 键盘弹跳 `CAKeyframeAnimation(values:[0,amp,0], keyTimes:[0,0.42,1], 0.16s)` → tick callback 线性插值 +
  `gtk_widget_set_transform`/`GskTransform` 平移，或对图片部件用 `margin`/`translate` 属性。
- 现有 `PetMotionPhysics`/`PetPointerAvoidance` 的 60Hz 定时器循环（`Timer`+`RunLoop.main`）
  → `gtk_widget_add_tick_callback`（vsync 驱动，更稳）。

### 4.6 其余平台替换

- **托盘**：KDE 走 StatusNotifierItem。优先 `libayatana-appindicator`（GTK3 库，可与 GTK4 进程共存），
  次选自实现 SNI over GDBus（无额外依赖，代码量约 200 行）。菜单项与 macOS 版一一对应
  （设置/隐藏宠物/始终置顶/位置锁定/位置重置/尺寸/图像/键输入状态/退出）；"输入监控权限"项改为
  "键输入权限状态 + 安装说明"。
- **设置窗口**：`AdwPreferencesWindow` + `AdwViewStack`/`AdwViewSwitcher` 三页
  （일반/갤러리/키 반응 → 中文界面可一并本地化），滑杆/开关/下拉/卡片网格与现有布局一一对应，
  按键捕获用 `GtkEventControllerKey` 抓一次按键（不再需要"抓取 sheet"这套 AppKit 特殊处理）。
- **持久化**：JSON（`~/.config/typingpet/config.json` + 现有 `imageGalleryData`/`keyReactionRules`
  的 JSON 结构直接复用），`UserDefaults` 抽成 `SettingsStore` 协议（macOS 实现用 `UserDefaults`，Linux 用 JSON）。
- **目录**：`~/.local/share/typingpet/Images/Sets`、`…/KeyReactions`（对齐 XDG）；内置资源放
  `share/typingpet/`（Flatpak 为 `/app/share/typingpet/`）。
- **自启动**：写/删 `~/.config/autostart/typingpet.desktop`，设置页开关直接反映文件是否存在。

---

## 5. 目标架构

```
TypingPet/
├─ Sources/
│  ├─ TypingPetCore/          # 纯 Foundation，Linux/macOS 双端可编译、可单测
│  │   ├─ PetGeometry.swift        # ← PetWindowGeometry.swift 平移
│  │   ├─ ImagePicker.swift        # ← 原样
│  │   ├─ ImageLibrary.swift       # 目录规则 + 图库集模型（去掉 NSImage/Bundle）
│  │   ├─ KeyModel.swift           # KeyStroke/KeyModifiers（evdev code）+ 显示名解析协议
│  │   ├─ ReactionStore.swift      # 规则 CRUD（存储后端注入）
│  │   └─ SettingsStore.swift      # protocol + 键值语义（scale/opacity/shake/…）
│  ├─ TypingPetMac/           # 现有 AppKit/SwiftUI 代码（保留发布能力）
│  └─ TypingPetLinux/         # 新 GTK4 平台层
│      ├─ PetWindow.swift          # layer-shell/X11 悬浮窗 + 输入区域切换
│      ├─ PetAnimation.swift       # tick 驱动的弹簧/弹跳/惯性/躲避
│      ├─ TrayItem.swift           # SNI 托盘 + 菜单
│      ├─ SettingsWindow.swift     # AdwPreferencesWindow 三页
│      ├─ InputSource.swift        # 抽象：EvdevInput / X11Input / PortalInput / MockInput
│      └─ main.swift
└─ Tests/TypingPetCoreTests/  # 现有 4 个测试文件迁入 + 新增 InputSource/SettingsStore 测试
```

原则：**核心库零平台导入**（`Foundation` only），`InputSource` 与 `SettingsStore` 都是协议，
真机输入、X11、portal、Mock 四种实现可插拔 —— 这样容器里也能跑回归测试。

---

## 6. 技术栈选型（已选 S1，见 §10）

| 方案 | 复用 Swift 核心 | 绑定成熟度 | 本机可开发性 | 打包 | 总评 |
|---|---|---|---|---|---|
| **S1. Swift + SwiftGtk(`gtk4` 分支)** | ✅ 直接 import | ⚠️ gir2swift 构建期生成、依赖链长、Swift 6 并发需适配（仓库 main 分支 2026-05 仍在更新，支持到 GTK 4.22） | ⚠️ 需 `gtk4-devel`+`gobject-introspection` 头文件（本容器无 root，需宿主 `dnf install`） | 需带 Swift runtime 或自建 Flatpak | **语言延续最佳，绑定风险中高** |
| **S2. Swift 核心 + 手写 C shim 直连 GTK4** | ✅ | ✅ 用稳定 C API，无第三方生成器 | 同 S1（需头文件） | 同上 | 依赖最少，但样板代码多（信号/闭包胶水约 400–600 行） |
| **S3. 全量重写为 Rust + gtk4-rs** | ❌（逻辑逐行照抄，约 600 行） | ✅ 生态最全（evdev/x11rb/gtk4-layer-shell/tray） | ✅ cargo 可直接装依赖（无需 root） | 单文件二进制，Flatpak/AppImage 最省事 | **长期最稳，但要重写 UI** |
| **S4. 全量重写为 Python + PyGObject** | ❌ | ✅ 稳定 | ✅ **本容器当下就能跑 GTK4 窗口**（已实测） | Flatpak 打包简单，需 Python runtime | 原型最快，运行时依赖略重 |

**我的建议（已采纳）**：走 **S1 为主、S2 兜底**的"Swift 优先"路线——因为它保留了这个仓库作为 Swift 项目的身份，
且 §2.3 已证明核心库能原生跑在 Linux 上；用 **P2 输入层 spike + P3 悬浮窗 spike 作为关卡**，
若 SwiftGtk 在 2 天内无法稳定画出"透明置顶 + 点击穿透 + 60fps 动效"，
再切 S3（Rust）或 S4（Python），此时核心逻辑已完成、行为规格已冻结，重写代价可控。

**实际工程形态是混合式（S1+S2）**：SwiftGtk 只覆盖 GTK/GDK/GObject，本方案还需要 4 个它不提供的库，
一律用 SwiftPM `.systemLibrary` + 手写小 shim 直连 C API：

| 库 | 用途 | 做法 |
|---|---|---|
| `libevdev` + `libudev` | evdev 按键读取、设备枚举与热插拔 | `systemLibrary(pkgConfig: "libevdev")` + `libudev` |
| `gtk4-layer-shell` | Wayland 置顶/锚定/键盘交互模式 | `systemLibrary(pkgConfig: "gtk4-layer-shell-0")` |
| `libayatana-appindicator3` | StatusNotifierItem 托盘 | 或改用 GDBus 自实现 SNI（无额外依赖） |
| `libxkbcommon` / `libX11`+`libXtst` | 键名解析；X11 光标轮询与 XRecord 后备 | `xkbcommon`、`x11`、`xtst` |

另外，Linux 目标建议沿用仓库现有的 `.swiftLanguageMode(.v5)`，避开 Swift 6 严格并发与
gir2swift 生成类型（非 `Sendable`、无 `@MainActor` 标注）之间的冲突。

---

## 7. 分阶段实施计划

| 阶段 | 内容 | 交付/验收标准 | 预估 |
|---|---|---|---|
| **P0 工程准备** | 抽 `TypingPetCore` target；`Package.swift` 加 `#if os(Linux)` 目标切分；`CGVector` shim；GitHub Actions 增 Linux job | `swift build` 在 macOS 与 Linux 均通过；macOS 发布脚本不受影响 | 1–2 天 |
| **P1 核心库移植** | 平移几何/物理/挑图/目录规则；`SettingsStore` 协议 + JSON 实现；`KeyModel` 改 evdev code；补单测 | Linux 上 `swift test` 全绿（现有 4 个测试文件 + 新增 ~10 用例） | 2–3 天 |
| **P2 输入层 spike（关卡 1）** | `typingpet-input-probe` CLI：evdev 枚举/热插拔/修饰键推导；X11 XRecord 备选；udev 规则样例 | 真机上打印出"任意按键 + 修饰键组合"；X11 下确认组合键可用、裸字母不可用（验证 §4.1 判断） | 3–5 天 |
| **P3 宠物窗口（关卡 2）** | layer-shell/X11 悬浮窗、点击穿透开关、图片显示+动图、透明度、拖拽/惯性/缩放/悬停控制、弹跳、躲避 | 三个会话类型下行为矩阵达标（下表）；动效 ≥55fps 掉帧可接受 | 5–8 天 |
| **P4 托盘 + 设置窗口** | SNI 托盘与菜单；`AdwPreferencesWindow` 三页；按键捕获；自启动 | 所有设置项与 macOS 版一一对应并能实时生效 | 4–6 天 |
| **P5 打包分发** | Flatpak manifest（GNOME runtime）+ `.desktop` + 图标 + udev 规则文档/安装脚本 + README(Linux) | 干净 Fedora 上 `flatpak install` 后开箱可用；键输入权限有明确指引 | 3–4 天 |
| **P6 验收矩阵** | X11 / KDE-Wayland / GNOME-Wayland 三档行为验证；性能与长稳（连打 10 分钟不泄漏） | 见 §7.1 | 2–3 天 |

### 7.1 行为验收矩阵（P6）

| 功能 | X11 | KDE Wayland | GNOME Wayland |
|---|---|---|---|
| 任意按键触发 | ✅ evdev | ✅ evdev | ✅ evdev |
| 组合键专属图 | ✅ | ✅（evdev；X11 后备亦可） | ✅ |
| 始终置顶 | ✅ `_NET_WM_STATE_ABOVE` | ✅ layer-shell | ✅ layer-shell |
| 点击穿透 | ✅ input shape | ✅ 空输入区域 | ✅ |
| 悬停透明度/控制按钮 | ✅ | ✅ | ✅ |
| 躲避光标 | ✅ `XQueryPointer` 完整 | ✅ 局部躲避（40px 检测环，已实现） | ✅ 局部躲避（同上） |
| 托盘 | ✅ | ✅ SNI | ⚠️ 需 AppIndicator 扩展 |
| 开机自启 | ✅ | ✅ | ✅ |
| 拖拽惯性/缩放/弹跳 | ✅ | ✅ | ✅ |

---

## 8. 风险登记册

| # | 风险 | 影响 | 缓解 |
|---|---|---|---|
| R1 | Wayland 无法全局监听键盘 | 核心玩法失效 | evdev 主路径 + udev 规则；容器内无 `/dev/input`，必须在宿主验证 |
| R2 | 用户不愿授予输入设备权限 | 功能不可用 | 三级权限方案；无权限时进入"仅组合键/仅测试按钮"降级模式并在 UI 说明 |
| R3 | Wayland 无法读取全局光标 | 躲避功能变弱 | 已定：X11 完整、Wayland 局部躲避 + UI 说明（§4.3） |
| R4 | SwiftGtk 绑定不稳 / 需新装头文件 | 阻塞 UI 开发 | P3 关卡 + 切 S3/S4 的预案；核心库已提前完成 |
| R5 | KWin 的 layer-shell 行为差异（层级/键盘焦点） | 悬浮窗不可用 | P2/P3 真机验证；备选"无边框 xdg-toplevel + `set_keep_above`（Wayland 不支持时退化为普通窗）" |
| R6 | 图像格式覆盖缩水（HEIC/APNG） | 与 README 承诺不符 | 明确依赖 `libheif-gdk-pixbuf`、`libwebp-pixbuf-loader`；不支持格式给出明确错误提示 |
| R7 | Flatpak 沙箱内读 `/dev/input` 与写 autostart 受限 | 打包版功能缺失 | manifest 里 `--device=all`（或 restricted）与 `--filesystem=xdg-config/autostart`；或提供 RPM/AppImage 分发 |
| R8 | 双平台同仓库导致构建回归 | 影响现有 macOS 发布 | CI 加 macOS job（现状保留）+ Linux job；核心库双端编译门禁 |

---

## 9. 测试与 CI

- **逻辑层**：现有 4 个测试文件迁入 `TypingPetCoreTests`，新增
  `SettingsStore` 读写、`KeyModel` evdev 映射、`InputSource`（Mock 事件序列 → 期望的状态机输出，含长按/重复键/组合键释放顺序）。
- **输入层**：`MockInputSource` 在 CI 里驱动全链路（无设备也能测"按键 → 挑图 → 0.75s 回 idle"）。
- **窗口层**：GTK 冒烟测试（`gtk_init` + 构造窗口 + 断言属性），在 CI 用 `xvfb-run` 或 Weston headless。
- **CI**：GitHub Actions 双 job —— `ubuntu-latest`（`apt install gtk4-dev libadwaita-1-dev libevdev-dev libxkbcommon-dev`）跑核心测试+构建；
  `macos-15`（现状）跑原测试。
- **真机手测清单**：三个会话类型 × §7.1 矩阵，每轮记录掉帧与内存（`/proc/<pid>/status` VmRSS）。

---

## 10. 决策记录

**已确认（2026-09-26）**

| # | 决策项 | 结论 |
|---|---|---|
| 1 | 技术栈 | ~~S1：Swift + SwiftGtk~~ → **已改为 S3：Rust + gtk4-rs**（理由：放弃 macOS 兼容后，SwiftGtk 的 5 个分支依赖、无版本 tag、需 gir 代码生成，稳定性明显不如 gtk4-rs；Linux 侧生态 crate 完备） |
| 2 | 目标会话 | **KDE Wayland 优先**（layer-shell 作默认悬浮窗实现）；X11 作为同权后备实现，运行时探测 `GdkDisplay` 自动切换 |
| 3 | 键输入权限 | **随包安装 udev `TAG+="uaccess"` 规则**（`60-typingpet-input.rules`），evdev 为主路径，X11 XRecord 为无权限后备 |
| 4 | 躲避光标 | **X11 完整（`XQueryPointer`）；Wayland 局部躲避（外扩输入区域）+ 设置页明确说明差异** |

**待确认**

| # | 决策项 | 默认走向 |
|---|---|---|
| 5 | 分发形态 | 先按 **Flatpak（GNOME runtime）优先**准备 P5；若你更想要 Fedora RPM / AppImage，P5 前告知即可 |

**环境前置条件（P3 之前必须解决）**

- 本开发容器：无 root、无 `gtk4-devel`/`libevdev-devel` 头文件、无 `/dev/input`。
  因此 **P0/P1（核心库抽取 + Linux 单测）可在此完成**；
  **P2 的 evdev 真机验证与 P3 的 GTK4 构建必须在宿主 Fedora 桌面**（`sudo dnf install …`，见附录 B），
  容器内可用 `MockInputSource` 与 headless 冒烟测试维持回归。

---

## 附录 A：建议目录结构

见 §5。

## 附录 B：Fedora 侧依赖与安装命令（宿主机执行，容器内无 root）

```bash
# 构建依赖
sudo dnf install -y gtk4-devel libadwaita-devel gobject-introspection-devel \
  libevdev-devel libxkbcommon-devel libX11-devel libXtst-devel \
  libgtk4-layer-shell-devel libayatana-appindicator-gtk3-devel
# 运行期图像 loader
sudo dnf install -y libheif-gdk-pixbuf libwebp-pixbuf-loader
# 键输入权限（二选一）
sudo install -m644 packaging/60-typingpet-input.rules /etc/udev/rules.d/ && sudo udevadm control --reload
# 或
sudo usermod -aG input "$USER"   # 需重新登录
```

## 附录 C：本次探索的复现命令

```bash
# 纯逻辑在 Linux 上的可移植性
swiftc -typecheck -module-cache-path .build/mcache Sources/TypingPet/ImagePicker.swift        # 通过
swiftc -typecheck -module-cache-path .build/mcache Sources/TypingPet/PetWindowGeometry.swift  # 仅缺 CGVector
swift build                                                                                    # no such module 'AppKit'
# GTK4 运行时与 Wayland 连通性
python3 -c "import gi; gi.require_version('Gtk','4.0'); from gi.repository import Gtk; Gtk.Window().present()"
```

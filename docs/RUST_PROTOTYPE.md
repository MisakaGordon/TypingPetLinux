# TypingPet for Linux（Rust 原型）

macOS 版 TypingPet 的 Linux 原生实现，用 **Rust + GTK4** 重写。本目录是功能验证原型：
**功能优先，UI 刻意从简**（没有复刻原版的状态栏菜单与三标签页设置窗口）。

```
crates/
├─ typingpet-core/     纯逻辑：几何/惯性物理/躲避、随机挑图、图库与目录规则、按键模型、配置
├─ typingpet-input/    evdev 全局按键（+ mock）、X11 光标查询、typingpet-probe 探针
└─ typingpet/          GTK4 宠物窗口 + 托盘 + headless 模式
packaging/             udev 规则、.desktop
```

## 1. 已实现的功能

| 功能 | 状态 |
|---|---|
| 任意按键触发随机反应图（不重复上一张） | ✅ evdev |
| 特定按键/组合键专属图（精确匹配优先于随机） | ✅ |
| 0.75 秒无输入回待机图 | ✅ |
| 待机/悬停透明度（默认 100% / 30%，立即生效） | ✅ |
| 按压力度分级的弹跳动画 | ✅ |
| 滚轮缩放（0.35–1.25） | ✅ 替代原版的拖拽缩放手柄 |
| 拖拽移动（layer-shell margin） | ✅ |
| 始终置顶 + 点击穿透（位置锁定） | ✅ layer-shell + 空输入区域 |
| 托盘菜单（StatusNotifierItem） | ✅ 设置/隐藏显示/置顶/点击穿透/位置重置/大小子菜单/键输入状态/退出 |
| 设置窗口（一般 / 图库 / 键反应 三页） | ✅ 纯 GTK4，改动即时生效并落盘 |
| 键反应规则的增删（选图 + 按一个键捕获） | ✅ 用全局输入流捕获，Wayland 下同样可用 |
| 图库管理（导入文件夹 / 应用 / 重命名 / 删除） | ✅ 缩略图、反应图张数、内置标记 |
| 开机自启 | ✅ 写/删 `~/.config/autostart/typingpet.desktop` |
| 图库：文件夹导入规则（`idle.*`/`pet-idle.*`） | ✅ 核心逻辑已实现并可单测 |
| 躲避光标 | ✅ X11：全局光标 + 90px 提前量；**Wayland：40px 检测环局部检测** |
| 悬停透明度 | ✅ X11 轮询全局光标 / Wayland 用界面事件（检测环内即可判定） |
| 动图（GIF / 动态 WebP） | ✅ 走 `GtkMediaFile`（GTK 内置图像动画后端，本身是 GdkPaintable） |
| 惯性滑行/边界反弹 | ✅ 核心逻辑已实现并单测 |

## 2. 构建与运行

### 2.1 依赖（Fedora）

```bash
sudo dnf install -y gtk4-devel libgtk4-layer-shell-devel
# 运行期：键输入需要权限（见 §3）
```

### 2.2 构建 / 运行

> ⚠️ 常见踩坑：`cargo run -p typingpet-input` 跑的是**键输入探针**（只打印按键、不开窗口），
> 宠物窗口在 `typingpet` 这个包里。为免混淆，仓库已配好 cargo 别名。

```bash
cargo pet                  # = cargo run --release -p typingpet  → 宠物窗口
cargo probe                # = 键输入探针（验证权限用，不开窗口）
cargo pet -- --help        # 给主程序传参数
```

等价的手写命令：

```bash
cargo build --release -p typingpet
./target/release/typingpet                 # 启动宠物窗口（GTK4 + 托盘）
./target/release/typingpet --help          # 全部参数
```

常用参数：

```
--headless          不开窗，在终端打印状态机动作（容器/CI 验证）
--mock[=N]          脚本化按键输入源，无需键盘权限（默认 12 次）
--scale F           覆盖缩放
--click-through     启动即鼠标穿透
--layer L           layer-shell 层级：top(默认) | overlay | bottom
--no-layer-shell    强制普通无边框窗口
--no-tray           不注册托盘
--print-events      打印每次检测到的按键（调试）
--dump-png PATH     启动 2 秒后把窗口内容直接渲染成 PNG（自检，绕过合成器）
--config / --data   配置文件与数据目录
--settings          启动时直接打开设置窗口
--settings-tab T    设置窗口初始页：general | gallery | keys
--position X,Y      强制宠物左上角坐标（屏幕逻辑像素）
--reset-position    忽略已保存的位置，回到主显示器右下角
--avoid-pointer     启动即开启「位置锁定中躲避光标」
--simulate-pointer X,Y  把光标假装在 (X,Y)（调试躲避逻辑，Wayland 下无法真实移动光标）
```

> 注意：`--click-through` / `--avoid-pointer` / `--scale` 只影响**本次会话**，不会被写进配置文件
> （只有你在设置窗口/托盘里主动改的值才会落盘）；`--position` 只决定初始位置，
> 之后的拖拽与躲避结果会正常保存。

### 2.3 键输入探针（先验证权限，再跑主程序）

```bash
cargo probe -- --list    # 列设备
cargo probe -- 10        # 监听 10 秒，打印按键
```

探针能读到按键 ≠ 主程序在跑：探针只是权限自检，它不会显示宠物。

### 2.4 打包与安装（tarball）

```bash
sh packaging/build-tarball.sh        # → dist/typingpet-<版本>-<架构>.tar.gz (+ .sha256)
sh install.sh --check                # 兼容性检查（glibc / GTK4 / layer-shell）
sh install.sh [--system] [--prefix DIR] [--no-udev] [--uninstall] [--purge]
```

包内容：`bin/typingpet`、`lib/libgtk4-layer-shell.so.0`、`.desktop`、udev 规则、`install.sh`、`BUILD-INFO.txt`。

设计要点：

- **附带 layer-shell**：`gtk4-layer-shell` 是**硬依赖**（ELF 的 `NEEDED`），很多发行版没有这个包，
  缺了连启动都不行。二进制加了 `RUNPATH=$ORIGIN/../lib`，安装脚本在系统已有该库时跳过附带那份。
- **兼容性前置检查**：glibc 版本用 `sort -V` 比较并明确拒绝过旧系统，
  避免用户看到 `version 'GLIBC_2.39' not found` 这种天书；GTK4 缺失时给出发行版对应的安装命令。
- **双模式**：系统级（`/usr/local` + 装 udev 规则）/ 用户级（`~/.local`，只打印需要 root 的那一步）
  —— 原子发行版（Silverblue / Bazzite / SteamOS）里 `/usr/local` 只读，只能走用户级。

实测（容器内）：

| 项 | 结果 |
|---|---|
| tarball 构建 | ✅ 6.3MB（含 9.4MB 二进制），附带 layer-shell 56KB |
| `install.sh --check` | ✅ 正确识别 Fedora / glibc 2.43 / GTK4 / layer-shell |
| 安装 → 运行 | ✅ 已安装的二进制正常启动，`layer-shell: 已启用` |
| 附带库解析 | ✅ `ldd` 显示解析到 `<prefix>/bin/../lib/libgtk4-layer-shell.so.0`（RUNPATH 优先于系统路径） |
| 版本比较 | ✅ 2.43/2.39 放行，2.36/2.35 明确拒绝 |
| 卸载 | ✅ 程序文件移除，配置与图片集保留（`--purge` 才清） |

### 2.5 多发行版构建（GitHub Release 分发）

glibc 与 GTK 的版本是**构建期**绑定的：在旧基线里编译出的二进制能在更新的系统上跑（反之不行）。
所以要覆盖多发行版，就要**按基线分别构建**，而不是一个二进制通吃。

为此把代码的 GTK 依赖从 4.12 降到了 **4.6**（否则旧基线根本编译不过 —— gtk4-rs 的 feature
在构建期就要求头文件版本）：

| 原 API | 版本 | 换成 | 版本 |
|---|---|---|---|
| `CssProvider::load_from_string` | 4.12 | `load_from_data` | 4.0 |
| `FileDialog`（选图/选文件夹） | 4.10 | `FileChooserNative`（同样走 portal） | 4.0 |
| `AlertDialog`（确认/报错） | 4.10 | `MessageDialog` | 4.0 |
| `Picture::set_content_fit` | 4.8 | 直接删掉（GtkPicture 默认就是 CONTAIN） | — |
| `Surface::scale`（分数缩放） | 4.12 | `scale_factor`（整数） | 4.0 |

最终二进制的 GTK 版本化符号里**不再出现** 4.8/4.10/4.12 的 API（用 `readelf --dyn-syms` 逐一核对）。

流水线：

```bash
sh packaging/build-matrix.sh          # 本地 podman/docker，多基线各出一份产物
```

- `packaging/container-build.sh`：在目标发行版容器内装依赖 → 装 Rust → **缺 gtk4-layer-shell 就从源码构建**
  → 调 `build-tarball.sh` 打包 → headless 自检 + `ldd` 检查未解析库。
- `packaging/build-matrix.sh`：宿主侧编排，镜像源可换（docker.io 不通时默认走 `docker.m.daocloud.io`）；
  挂载用 `:Z` 以适配 SELinux Enforcing 的宿主。
- `.github/workflows/release.yml`：打 tag 时用 `container:` 在 5 个基线里并行构建并发布 Release。

## 3. 键输入权限（Wayland 没有 `CGEventTap` 等价物）

macOS 用 `CGEventTap` 全局监听；Linux 上要在 **X11 与 Wayland 都拿到"任意按键"**，只能用 evdev 直读
`/dev/input/event*`。装一条 udev 规则即可（仅对当前登录用户授予 ACL，登出失效）：

```bash
sudo install -m644 packaging/60-typingpet-input.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=input
```

替代方案（安全性更低）：`sudo usermod -aG input "$USER"` 后重新登录。

隐私：只读取 `EV_KEY` 的键码与按下/抬起，用于推导修饰键；不读字符、不落盘、不联网。

## 3.1 Wayland 兼容性（已完整落地）

Wayland 出于安全设计不提供 macOS `CGEventTap` 那套能力，本项目用平台允许的方式逐一补齐：

| 能力 | Wayland 下的做法 | 状态 |
|---|---|---|
| 全局键盘（任意按键） | evdev 直读 `/dev/input/event*` + udev `uaccess` | ✅ 与 X11 等效 |
| 始终置顶 | `gtk4-layer-shell` 的 `Layer::Top`（X11 下退化为 `_NET_WM_STATE_ABOVE`） | ✅ KWin/Wayland |
| 窗口定位 | layer-shell `anchor(Top\|Left)` + `margin` | ✅ |
| 鼠标点击穿透 | `wl_surface.set_input_region` 设成空集 | ✅ |
| 悬停透明度 | 界面事件（检测环内即可判定），不依赖全局光标 | ✅ |
| 躲避光标 | **40px 检测环**：窗口比图片每边大 40px，锁定时把环放进输入区域，靠环内的指针事件判定并弹开 | ✅ 局部 |
| 托盘 | StatusNotifierItem（D-Bus） | ✅ |
| 设置窗口 / 文件选择 | 普通 xdg-toplevel + xdg-desktop-portal | ✅ |
| 开机自启 | XDG autostart | ✅ |

**代价与说明**：Wayland 的躲避是"靠近才躲"（40px），而不是 X11 的 90px 提前量；
且"锁定 + 躲避"开启期间，那一圈 40px 会占用鼠标点击（关闭躲避即恢复完全穿透）。
这条已在设置窗口里写明。

**已知平台限制**：GNOME (Mutter) 不支持 `wlr-layer-shell`，因此 GNOME Wayland 下无法置顶/定位，
程序会打印提示并退化为普通无边框窗口；X11 与 KDE Wayland 不受影响。

### X11 支持

| 能力 | X11 下的做法 |
|---|---|
| 窗口定位 / 拖动 / 躲避推动窗口 | GTK4 **没有**任何窗口定位 API，只能 `gdk4-x11` 取窗口 id + `x11rb` 发 `ConfigureWindow`；layer-shell 的 `set_margin` 只在 Wayland 有效 |
| 坐标换算 | `state.position` 是 GDK 逻辑像素，X11 root 是物理像素 → 移动窗口时乘回 scale（与读光标时的除法互逆，有单测钉住） |
| 映射后的位置 | 窗口刚映射时窗口管理器会自行摆放，前 ~0.5s 反复重申目标位置 |
| 点击穿透 | `gdk_surface_set_input_region` 在 X11 下映射为 input shape，同样有效 |
| 全局光标 | `XQueryPointer`（连接复用，不再每次重连） |

**注意**：在 **Wayland 会话 + X11 后端（XWayland）** 这个组合下，即使定位能用，
也存在两个平台限制：没有 layer-shell（不能置顶），且 KWin 默认不向 X11 客户端提供光标位置
（`XwaylandEavesdropsMouse=false`），导致躲避失效。程序启动时会打印警告建议改用原生 Wayland 后端。

### 动图是怎么实现的

- **判定**：`GdkPixbufAnimation::from_file` + `is_static_image()`，只对动图走特殊路径。
- **播放**：`GtkMediaFile::for_filename` + `set_loop(true)` + `play()`，直接当作 `GtkPicture`
  的 paintable —— `GtkMediaFile` 本身实现 `GdkPaintable`，GTK 自己按帧时钟推进，
  **不需要手写逐帧定时器，也不依赖 GStreamer**（GTK 有内置图像后端）。
- **静图**：仍走预缩放 pixbuf（快，且保证窗口尺寸 == 设定尺寸）。
- **缩放变化**：动图只改尺寸请求、不重新加载（否则滚轮每格都会重启动画）。
- **回退**：动图播放出错（缺 loader）时经 `notify::error` 回退成静态首帧，宠物不会整个消失。
- **依赖**：动图能力来自 gdk-pixbuf 的 loader（Fedora 44 用 glycin 后端；其他发行版上
  动态 WebP 可能需要 `libwebp-pixbuf-loader`）。缺 loader 时自动退化为静帧，不会报错。

实测：待机图换成 3 帧 GIF 后连拍 4 张，宠物区域签名与平均色在变（0.853↔0.822 交替）；
换成静态 PNG 的对照组 3 张签名完全一致。

## 4. 与 macOS 版的差异

| 项 | macOS 版 | 本原型 |
|---|---|---|
| 键码空间 | macOS 虚拟键码（`40` = K） | evdev 键码（`37` = K），配置不互通 |
| 窗口机制 | `NSPanel` + `ignoresMouseEvents` | layer-shell（Wayland）/ 普通无边框窗口（X11） |
| 缩放 | 右上角手柄拖拽 | 滚轮（无手柄控件） |
| 设置 | 三标签页 SwiftUI 窗口 | 三标签页 GTK4 窗口（信息架构一致，控件更朴素） |
| 菜单 | 状态栏菜单 20+ 项 | 托盘菜单 12 项（设置/显隐/置顶/穿透/重置/大小/状态/退出） |
| 躲避光标 | 全局光标，全距离预判 | X11：等价实现（90px）；Wayland：40px 检测环局部检测（协议限制） |
| 动图 | `NSImageView.animates` | ✅ 支持（GIF / 动态 WebP） |
| 登录自启 | `SMAppService` | 未接（`~/.config/autostart/*.desktop`） |

## 5. 已知缺口 / 下一步

1. **已输入字符的读取**：不做，也不打算做（隐私承诺）。
3. **打包**：tarball + `install.sh` 已完成（见 §2.4）；Flatpak / RPM 仍待做。
   要覆盖 glibc < 2.39 的老发行版（Debian 12 / Ubuntu 22.04 / RHEL 9），需要在旧基线容器里构建。
4. **设置窗口细节**：图片集导入进度提示、规则按键冲突提示、界面多语言（当前中文硬编码）。

## 6. 本容器（无 root / 无 GTK4 头文件）里的构建方式

宿主 Fedora 装好 `gtk4-devel` 后直接 `cargo build` 即可；若在受限容器里构建，可把开发头文件解包成
本地 sysroot，并给链接器提供无版本号 `.so`：

```bash
dnf download --destdir .build/rpms --resolve --alldeps gtk4-devel gtk4-layer-shell-devel
cd .build/sysroot && for r in ../rpms/*.rpm; do rpm2cpio "$r" | cpio -idm --quiet; done
mkdir -p ~/gtklibs && for l in gtk-4 gtk4-layer-shell pangocairo-1.0 pango-1.0 gdk_pixbuf-2.0 \
  harfbuzz vulkan graphene-1.0 gio-2.0 gobject-2.0 glib-2.0 cairo cairo-gobject; do
  cp -L "$(ls /lib64/lib$l.so.* | head -1)" ~/gtklibs/lib$l.so; done

export PKG_CONFIG_SYSROOT_DIR=$PWD/.build/sysroot
export PKG_CONFIG_PATH=$PWD/.build/sysroot/usr/lib64/pkgconfig:$PWD/.build/sysroot/usr/share/pkgconfig
export LIBRARY_PATH=$HOME/gtklibs
cargo build -p typingpet
```

## 7. 验证记录（本原型实测）

| 验证项 | 命令 | 结果 |
|---|---|---|
| 核心逻辑单测（含 macOS 版 4 个测试文件的移植） | `cargo test -p typingpet-core` | ✅ 34 passed |
| 输入层单测 | `cargo test -p typingpet-input` | ✅ 3 passed |
| headless 全链路 | `typingpet --headless --mock=6 --print-events` | ✅ 按键→换图→弹跳→0.75s 回待机 |
| Wayland 窗口 | `typingpet --mock=1` | ✅ surface 453x453 mapped，`layer=top`（KWin 支持 layer-shell） |
| 渲染自检（离屏 `render_texture`） | `typingpet --dump-png /tmp/x.png` | ⚠️ 本容器内不稳定（见下） |
| 真实桌面可见 | 运行时 `spectacle` 截屏 | ✅ 宠物浮在普通窗口之上（layer-shell Top 生效） |
| evdev 真机按键 | `cargo probe -- 10` | ✅ 用户宿主实测可读到按键 |
| 默认参数启动可见性 | `./target/release/typingpet`（不带参数） | ✅ 右下角可见，浮于窗口之上 |
| X11 定位 | `GDK_BACKEND=x11` + 配置 `pet_x/pet_y=400/300` | ✅ `diag(x11)` 实际位置 =(400,300) |
| X11 躲避推动窗口 | X11 + `--avoid-pointer --simulate-pointer 420,500` | ✅ 窗口被真实移动到 (497,317) |
| X11 指针解耦（限制） | `--warp X,Y` 两次不同坐标 | ⚠️ 位置不变（KWin 忽略 XWayland 的 warp），已加启动警告 |
| 设置窗口 · 一般页 | `typingpet --settings` | ✅ 滑杆/开关/下拉/状态行渲染正确 |
| 设置窗口 · 图库页 | `--settings-tab gallery`（预置 2 个图片集） | ✅ 缩略图、反应张数、内置标记 |
| 设置窗口 · 键反应页 | `--settings-tab keys`（预置 Ctrl+K / Space） | ✅ 键名显示正确（evdev 37 + Ctrl → `Ctrl+K`） |
| 托盘注册 | 默认启动 | ✅ `tray: 已注册 StatusNotifierItem` |
| 编译警告 | `cargo build --release` | ✅ 0 warning |
| Wayland 躲避（模拟光标） | `--click-through --avoid-pointer --position 600,400 --simulate-pointer 610,500` | ✅ 窗口 (600,400) → (627,419)，移出检测环后停住 |
| Wayland 悬停 | `--position 500,400 --simulate-pointer 600,530` | ✅ `hover=true`，窗口不移动 |
| 锁定但不躲避 | `--click-through --position 600,400 --simulate-pointer 610,500` | ✅ 窗口不动（完全穿透） |
| CLI 覆盖不落盘 | 同一轮运行后检查配置文件 | ✅ `position_locked`/`avoids` 仍为 false |

> 实测踩坑记录：用户执行 `cargo run -p typingpet-input` → 跑的是**探针**，因此"能读到输入但桌面没有宠物"。
> 主程序命令是 `cargo pet`（见 §2.2）。

### 关于 `--dump-png` 在本容器里不稳定

本容器的 EGL/GL 是坏的（启动即报 `MESA-EGL: failed to create dri2 screen`、`ZINK: failed to choose pdev`），
GSK 的离屏 `render_texture()` 会间歇性返回空节点（`empty render node`），`GSK_RENDERER=cairo` 也一样；
但**合成器层面的渲染完全正常**（截屏可见宠物浮在窗口之上）。
所以：验证"图有没有画出来"请用截屏，`--dump-png` 只作为辅助手段，且它在正常桌面（GL 可用）上通常可用。

### 排查过程中修掉的三个真实缺陷

1. **图片被分配成 0×0**：`GtkPicture` 设了 `can_shrink(true)` 后自然尺寸为 0，而 `GtkFixed` 按子控件
   自然尺寸分配 → 窗口正常映射、`paintable` 非空，但**什么都不画**。修法：给图片显式 `set_size_request`。
   （这个缺陷只有真的去截屏/自渲染才会暴露。）
2. **X11 下的 GLib CRITICAL**：`gtk4-layer-shell` 的函数只能在 Wayland 显示上调用，
   `is_supported()` 在 X11 后端会触发断言告警。修法：先用 GDK 显示类型名判断是否 Wayland。
3. **悬停时桌宠疯狂闪烁（窗口像在快速出现/消失）**：光标来源混用了两套坐标系 ——
   X11 root 是**物理像素**（2160x1440），窗口与宠物框是 GDK **逻辑像素**（1600x1067，分数缩放 1.35）。
   帧循环每 33ms 用 X11 坐标覆盖 `pointer`，而界面事件只在"还不是全局来源"时才写，
   于是"界面事件置为悬停(0.3)"与"X11 坐标判为未悬停(1.0)"以 30–60Hz 互相翻转。
   修法：**启动时定死光标来源**（Wayland 只认界面事件、完全不轮询 X11；X11 才轮询，
   且物理坐标按缩放换算回逻辑坐标），并给悬停判定加 6px 迟滞。
   教训：容器里用 `--simulate-pointer` 自测恰好绕开了这条路径，真机真鼠标才复现。

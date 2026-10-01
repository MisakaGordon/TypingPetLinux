[English](README.md)
# TypingPet for Linux

按键时会有反应的桌面宠物 —— **Rust + GTK4** 实现的 Linux 原生版本，在 KDE Plasma 6 / Wayland 上开发与验证。

> 本项目源自 macOS 版 [TypingPet](https://github.com/ynifamily3/TypingPetMac)（作者 MisakaGordon，Swift + AppKit/SwiftUI 实现）。
> **原作者的 macOS 版完整保留在 `macos-swift` 分支**，本分支（`main`）只包含 Linux 版。

按键时宠物会换成随机的反应图并弹一下，0.75 秒没输入就回到待机图；可以给特定键或组合键指定专属图片。

## 功能

- **任意按键触发**：随机换反应图（不会连续重复同一张）+ 按强度分级的弹跳动画
- **特定键反应**：精确匹配的规则优先于随机反应，支持 `Ctrl/Alt/Shift/Super` 组合
- **动图**：GIF / 动态 WebP 会真的动起来（待机图与反应图都支持）
- **待机回归**：0.75 秒无输入回待机图
- **透明度**：常驻 / 悬停两个值分别可调，立即生效
- **窗口行为**：始终置顶、位置锁定（鼠标点击穿透）、拖拽移动、滚轮缩放（0.35–1.25）
- **躲避光标**：光标靠近时宠物自己挪开（X11 90px 提前量；Wayland 为 40px 检测环局部检测）
- **开机自启**：写 `~/.config/autostart/typingpet.desktop`
- **托盘菜单**：设置 / 显隐 / 置顶 / 点击穿透 / 位置重置 / 尺寸预设 / 状态 / 退出
- **设置窗口**：一般 · 图库 · 键反应 三页
- **图库**：导入图片文件夹、切换/重命名/删除图片集
- **隐私**：只读取按键的 keycode 与按下状态，不读字符、不落盘、不联网

## 构建与运行

需要 Rust 1.75+ 与 GTK4 开发包：

```bash
# Fedora
sudo dnf install -y gtk4-devel gtk4-layer-shell-devel
# Ubuntu / Debian（gtk4-layer-shell 视发行版可能需要自行编译）
sudo apt install -y libgtk-4-dev libgtk4-layer-shell-dev

git clone https://github.com/MisakaGordon/TypingPetLinux.git
cd TypingPetLinux
cargo pet                  # = cargo run --release -p typingpet
```

> ⚠️ `cargo run -p typingpet-input` 跑的是**键输入探针**（只打印按键、不开窗口），
> 宠物窗口在 `typingpet` 包里。仓库已配好别名：`cargo pet`（主程序）/ `cargo probe`（探针）。

### 键输入权限（Wayland 没有 macOS `CGEventTap` 的等价物）

要在 X11 与 Wayland 下都拿到"任意按键"，只能直接读 `/dev/input/event*`。装一条 udev 规则即可
（只对当前登录用户授予 ACL，登出失效）：

```bash
sudo install -m644 packaging/60-typingpet-input.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=input
cargo probe -- 10        # 先确认能读到按键
```

替代方案（安全性更低）：`sudo usermod -aG input "$USER"` 后重新登录。

### 打包分发（tarball）

```bash
sh packaging/build-tarball.sh          # 产物在 dist/，含 sha256
tar xf dist/typingpet-*.tar.gz && cd typingpet-*/
sh install.sh --check                  # 先做兼容性检查，不改动系统
sh install.sh                          # 装到 ~/.local
sudo sh install.sh --system            # 装到 /usr/local（可顺带装 udev 规则）
sh install.sh --uninstall              # 卸载
```

包里附带 `libgtk4-layer-shell.so.0`（56KB）—— 多数发行版没有这个包，缺了程序起不来；
二进制用 `$ORIGIN/../lib` 找它，系统已有该库时安装脚本会自动跳过附带那份。

**兼容性下限**（`install.sh --check` 会明确检查并给出发行版对应的安装命令）：

| 项 | 要求 | 说明 |
|---|---|---|
| glibc | ≥ 2.39 | 覆盖 Fedora 40+ / Ubuntu 24.04+ / Debian 13+ / Arch / openSUSE TW |
| GTK4 | ≥ 4.12 | 需要运行库（Fedora 装 `gtk4`，Debian/Ubuntu 装 `libgtk-4-1`） |

> Debian 12（glibc 2.36 / GTK 4.8）、Ubuntu 22.04（2.35 / 4.6）、RHEL 9（2.34 / 4.6）
> 目前**不支持**：glibc 下限来自 zbus 的进程创建路径（已实测，见 `docs/RUST_PROTOTYPE.md`），
> 要覆盖它们需要在旧基线容器里构建，脚本会给出明确提示而不是链接错误。

### 常用参数

```
--headless          不开窗，在终端打印状态机动作（CI/容器验证）
--mock[=N]          脚本化按键输入源，无需键盘权限
--settings          启动时打开设置窗口（--settings-tab general|gallery|keys）
--scale F           本次会话缩放覆盖    --position X,Y   本次会话初始坐标
--click-through     启动即位置锁定（点击穿透）
--avoid-pointer     启动即开启躲避光标
--layer L           layer-shell 层级：top(默认) | overlay | bottom
--no-layer-shell    强制普通无边框窗口
--dump-png PATH     把窗口内容渲染成 PNG（自检）
```

> `--click-through` / `--avoid-pointer` / `--scale` 只影响**本次会话**，不会写进配置文件；
> 只有你在设置窗口或托盘里主动改的值才会落盘。

## Wayland 兼容性

Wayland 出于安全设计不提供 macOS 那套全局输入/窗口控制 API，本项目用平台允许的方式逐一补齐：

| 能力 | 做法 |
|---|---|
| 全局键盘（任意按键） | evdev 直读 `/dev/input/event*` + udev `uaccess`（X11/Wayland 等效） |
| 始终置顶 | `gtk4-layer-shell` 的 `Layer::Top` |
| 窗口定位 | layer-shell `anchor(Top\|Left)` + `margin` |
| 鼠标点击穿透 | `wl_surface.set_input_region` 设为空集 |
| 悬停透明度 | 界面事件 + 宠物本体矩形判定 |
| 躲避光标 | **40px 检测环**：窗口比图片每边大 40px，锁定时把环放进输入区域，靠环内指针事件弹开 |
| 托盘 / 设置窗口 / 文件选择 | StatusNotifierItem + 普通 xdg-toplevel + xdg-desktop-portal |

**代价**：Wayland 的躲避是"靠近才躲"（40px），且"锁定 + 躲避"期间那一圈会占用鼠标点击 ——
关闭躲避即恢复完全穿透。设置窗口里对这条有说明。

**平台限制**：GNOME (Mutter) 不支持 `wlr-layer-shell`，GNOME Wayland 下无法置顶/定位，
程序会提示并退化为普通无边框窗口。X11 与 KDE Wayland 不受影响。

## 代码结构

```
crates/
├─ typingpet-core/     纯逻辑：几何/惯性物理/躲避、随机挑图、图库与目录规则、按键模型、配置、状态机
├─ typingpet-input/    evdev 全局按键（+ mock）、X11 光标查询、typingpet-probe 探针
└─ typingpet/          GTK4 宠物窗口、设置窗口、托盘、headless 模式
packaging/             udev 规则、.desktop
```

设计要点：核心逻辑零 GUI 依赖，输入层与配置存储都是可替换的抽象，
因此全部逻辑都能在无头环境（CI/容器）里测试。`cargo test --workspace` 共 43 项。

## 许可证

- 源代码：[MIT](LICENSE)
- 内置键盘猫素材：[CC0 1.0](ASSET_LICENSE.md)

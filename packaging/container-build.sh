#!/bin/sh
# 在目标发行版容器内执行：装依赖 → 装 Rust → 构建 → 打包 → 自检。
# 由 packaging/build-matrix.sh（本地 podman）或 .github/workflows/release.yml（CI）调用。
#
# 用法: sh packaging/container-build.sh <apt|dnf|pacman|zypper>
set -eu

FAMILY=${1:?需要包管理器家族: apt | dnf | pacman | zypper}
GTK4_LAYER_SHELL_REPO=${GTK4_LAYER_SHELL_REPO:-https://github.com/wmww/gtk4-layer-shell}

echo "==> [$FAMILY] 安装构建依赖"
case "$FAMILY" in
    apt)
        export DEBIAN_FRONTEND=noninteractive
        # 部分地区直连官方源很慢；可用 APT_MIRROR 指定镜像，例如：
        #   APT_MIRROR=https://mirrors.tuna.tsinghua.edu.cn/debian
        if [ -n "${APT_MIRROR:-}" ]; then
            . /etc/os-release
            codename=${VERSION_CODENAME:?无法确定发行版代号}
            case "${ID:-}" in
                ubuntu) components="main universe" ;;
                *)      components="main" ;;
            esac
            {
                echo "deb $APT_MIRROR $codename $components"
                echo "deb $APT_MIRROR $codename-updates $components"
                echo "deb $APT_MIRROR $codename-security $components"
            } > /etc/apt/sources.list
            echo "    已切换到镜像源: $APT_MIRROR ($codename)"
        fi
        apt-get update -qq
        apt-get install -y -qq --no-install-recommends \
            curl git ca-certificates build-essential pkg-config binutils \
            meson ninja-build libgtk-4-dev libwayland-dev wayland-protocols
        ;;
    dnf)
        # 同样支持换源（Fedora 官方源在某些网络下很慢）：
        #   DNF_MIRROR=https://mirrors.tuna.tsinghua.edu.cn/fedora
        if [ -n "${DNF_MIRROR:-}" ] && [ -d /etc/yum.repos.d ]; then
            sed -i -e 's|^metalink=|#metalink=|' \
                   -e "s|^#baseurl=http://download.example/pub/fedora/linux|baseurl=$DNF_MIRROR|" \
                   /etc/yum.repos.d/*.repo 2>/dev/null || true
            echo "    已切换到镜像源: $DNF_MIRROR"
        fi
        dnf install -y -q curl git ca-certificates gcc gcc-c++ make pkgconf-pkg-config \
            binutils meson ninja-build gtk4-devel wayland-devel wayland-protocols-devel
        ;;
    pacman)
        pacman -Sy --noconfirm --needed curl git base-devel pkgconf binutils \
            meson ninja gtk4 wayland wayland-protocols
        ;;
    zypper)
        zypper -n install curl git ca-certificates gcc gcc-c++ make pkg-config binutils \
            meson ninja gtk4-devel wayland-devel wayland-protocols-devel
        ;;
    *) echo "未知包管理器家族: $FAMILY" >&2; exit 2 ;;
esac

echo "==> 安装 Rust 工具链（发行版自带的往往太旧）"
if ! command -v cargo >/dev/null 2>&1; then
    curl -sSf https://sh.rustup.rs \
        | sh -s -- -y --profile minimal --default-toolchain stable --no-modify-path
fi
# shellcheck disable=SC1091
. "$HOME/.cargo/env"

echo "==> 检查 gtk4-layer-shell（很多发行版没有这个包，缺了链接不过）"
if ! pkg-config --exists gtk4-layer-shell-0; then
    echo "    未安装：从源码构建（这一份随后会被打进 tarball）"
    git clone --depth 1 "$GTK4_LAYER_SHELL_REPO" /tmp/gtk4-layer-shell
    cd /tmp/gtk4-layer-shell
    meson setup build --prefix=/usr --buildtype=release
    ninja -C build
    ninja -C build install
    ldconfig 2>/dev/null || true
    cd - >/dev/null
fi
pkg-config --modversion gtk4-layer-shell-0 | sed 's/^/    gtk4-layer-shell /'
pkg-config --modversion gtk4 | sed 's/^/    gtk4 /'

echo "==> 构建并打包"
TARGET_SUFFIX=${TARGET_SUFFIX:-} sh packaging/build-tarball.sh

echo "==> 自检（不需要显示器）"
BIN=target/release/typingpet
[ -x "$BIN" ] || { echo "!! 没找到 $BIN" >&2; exit 1; }
rm -rf /tmp/smoke
if "$BIN" --headless --mock=3 --data /tmp/smoke | tail -3; then
    echo "    headless 自检通过"
else
    echo "!! headless 自检失败" >&2; exit 1
fi
"$BIN" --help >/dev/null && echo "    --help 正常"
echo "    动态依赖: $(ldd "$BIN" | grep -c 'not found') 个未解析（应为 0）"
ldd "$BIN" | grep -q 'not found' && { echo "!! 有未解析的动态库" >&2; exit 1; } || true

echo "==> 产物"
ls -1 dist/*.tar.gz*

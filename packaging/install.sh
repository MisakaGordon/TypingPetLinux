#!/bin/sh
# TypingPet for Linux —— 跨发行版安装/卸载脚本（POSIX sh）
#
#   sh install.sh                普通用户：装到 ~/.local
#   sudo sh install.sh --system  系统级：装到 /usr/local（可顺带装 udev 规则）
#   sh install.sh --check        只检查兼容性，不改动系统
#   sh install.sh --uninstall    卸载
#
# 选项: --prefix DIR  --no-udev  --purge  --force  -h|--help
set -eu

# ---------- 参数 ----------
MODE=auto
PREFIX=""
WITH_UDEV=yes
ACTION=install
PURGE=no
FORCE=no

while [ $# -gt 0 ]; do
    case "$1" in
        --user)      MODE=user ;;
        --system)    MODE=system ;;
        --prefix)    shift; PREFIX=${1:?--prefix 需要目录} ;;
        --no-udev)   WITH_UDEV=no ;;
        --uninstall) ACTION=uninstall ;;
        --purge)     PURGE=yes ;;
        --check)     ACTION=check ;;
        --force)     FORCE=yes ;;
        -h|--help)
            sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'
            exit 0 ;;
        *) echo "未知参数: $1（用 --help 查看）" >&2; exit 2 ;;
    esac
    shift
done

SRC=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
die() { echo "错误: $*" >&2; exit 1; }
info() { echo "  $*"; }

# ---------- 目标前缀 ----------
if [ -z "$PREFIX" ]; then
    if [ "$MODE" = system ] || { [ "$MODE" = auto ] && [ "$(id -u)" = 0 ]; }; then
        MODE=system; PREFIX=/usr/local
    else
        MODE=user; PREFIX=${XDG_DATA_HOME:-$HOME/.local}
        PREFIX=$(dirname -- "$PREFIX")   # ~/.local/share -> ~/.local
    fi
fi
BIN_DIR="$PREFIX/bin"
LIB_DIR="$PREFIX/lib"
APP_DIR="$PREFIX/share/applications"
UDEV_DEST=/etc/udev/rules.d/60-typingpet-input.rules

# ---------- 发行版信息 ----------
DISTRO_ID=""; DISTRO_LIKE=""
if [ -r /etc/os-release ]; then
    # shellcheck disable=SC1091
    . /etc/os-release
    DISTRO_ID=${ID:-}; DISTRO_LIKE=${ID_LIKE:-}
fi
distro_hint() {
    case " $DISTRO_ID $DISTRO_LIKE " in
        *fedora*|*rhel*)            echo "sudo dnf install -y gtk4" ;;
        *debian*|*ubuntu*|*apt*)    echo "sudo apt install -y libgtk-4-1" ;;
        *arch*)                     echo "sudo pacman -S --needed gtk4" ;;
        *suse*)                     echo "sudo zypper install -y gtk4" ;;
        *)                          echo "用你的包管理器安装 GTK4 运行库" ;;
    esac
}

# ---------- 兼容性检查 ----------
have_gtk4() {
    if command -v ldconfig >/dev/null 2>&1; then
        ldconfig -p 2>/dev/null | grep -q 'libgtk-4\.so\.1' && return 0
    fi
    for d in /usr/lib /usr/lib64 /usr/lib/*-linux-gnu "$PREFIX/lib"; do
        [ -e "$d/libgtk-4.so.1" ] && return 0
    done
    return 1
}
have_layer_shell() {
    if command -v ldconfig >/dev/null 2>&1; then
        ldconfig -p 2>/dev/null | grep -q 'libgtk4-layer-shell\.so\.0' && return 0
    fi
    return 1
}
glibc_version() {
    if command -v getconf >/dev/null 2>&1; then
        getconf GNU_LIBC_VERSION 2>/dev/null | awk '{print $2}'
    else
        ldd --version 2>/dev/null | head -1 | awk '{print $NF}'
    fi
}
# 版本比较：$1 >= $2 ?
version_ge() {
    [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -1)" = "$2" ]
}
BIN_FLOOR=$(sed -n 's/^glibc floor: *\([0-9.]*\).*/\1/p' "$SRC/BUILD-INFO.txt" 2>/dev/null || echo "")

check() {
    echo "== 环境检查 =="
    info "发行版    : ${DISTRO_ID:-未知} ${DISTRO_LIKE:+($DISTRO_LIKE)}"
    info "架构      : $(uname -m)"
    LIBGLIBC=$(glibc_version || echo "")
    if [ -n "$LIBGLIBC" ]; then
        if [ -n "$BIN_FLOOR" ] && ! version_ge "$LIBGLIBC" "$BIN_FLOOR"; then
            info "glibc     : $LIBGLIBC  ✗ 低于二进制要求的 $BIN_FLOOR"
            STATUS=bad
        else
            info "glibc     : $LIBGLIBC  ✓"
        fi
    fi
    if have_gtk4; then
        info "GTK4      : 已安装 ✓"
    else
        info "GTK4      : 缺失 ✗  →  $(distro_hint)"
        STATUS=bad
    fi
    if have_layer_shell; then
        info "layer-shell: 系统已有 ✓"
    elif [ -f "$SRC/lib/libgtk4-layer-shell.so.0" ]; then
        info "layer-shell: 系统没有，将使用随包附带的那份 ✓"
    else
        info "layer-shell: 缺失，且包内未附带 ✗（Wayland 下无法置顶）"
    fi
    [ "${STATUS:-ok}" = bad ] && return 1
    return 0
}

# ---------- 卸载 ----------
if [ "$ACTION" = uninstall ]; then
    echo "== 卸载（前缀 $PREFIX）=="
    rm -f "$BIN_DIR/typingpet" "$LIB_DIR/libgtk4-layer-shell.so.0" \
          "$APP_DIR/typingpet.desktop"
    if [ "$WITH_UDEV" = yes ] && [ -f "$UDEV_DEST" ]; then
        if [ "$(id -u)" = 0 ]; then rm -f "$UDEV_DEST"; info "已删除 $UDEV_DEST"
        else info "需要 root 才能删除 $UDEV_DEST：sudo rm -f $UDEV_DEST"; fi
    fi
    info "已移除程序文件（配置与图片集保留在 ~/.config/typingpet 与 ~/.local/share/typingpet）"
    if [ "$PURGE" = yes ]; then
        rm -rf "${XDG_CONFIG_HOME:-$HOME/.config}/typingpet" \
               "${XDG_DATA_HOME:-$HOME/.local/share}/typingpet"
        info "已清除配置与图片集（--purge）"
    fi
    command -v update-desktop-database >/dev/null 2>&1 && \
        update-desktop-database "$APP_DIR" 2>/dev/null || true
    exit 0
fi

# ---------- 检查 ----------
if [ "$ACTION" = check ]; then
    check && { echo "== 结论：可以直接安装 =="; exit 0; } \
          || { echo "== 结论：缺少依赖，请先按上面的提示安装 =="; exit 1; }
fi

# ---------- 安装 ----------
echo "== 安装 TypingPet（${MODE}，前缀 $PREFIX）=="
[ -x "$SRC/bin/typingpet" ] || die "找不到 $SRC/bin/typingpet（请在解压后的目录里运行）"

if ! check && [ "$FORCE" != yes ]; then
    die "环境检查未通过；确认要继续可加 --force"
fi

mkdir -p "$BIN_DIR" "$APP_DIR"
install -m755 "$SRC/bin/typingpet" "$BIN_DIR/typingpet"
info "已安装 $BIN_DIR/typingpet"

# 只有系统没有 gtk4-layer-shell 时才附带，避免遮蔽发行版更新的版本
if have_layer_shell; then
    info "系统已有 libgtk4-layer-shell.so.0，跳过随包那份"
elif [ -f "$SRC/lib/libgtk4-layer-shell.so.0" ]; then
    mkdir -p "$LIB_DIR"
    install -m644 "$SRC/lib/libgtk4-layer-shell.so.0" "$LIB_DIR/"
    info "已安装 $LIB_DIR/libgtk4-layer-shell.so.0（二进制通过 \$ORIGIN/../lib 找到它）"
fi

if [ -f "$SRC/share/applications/typingpet.desktop" ]; then
    install -m644 "$SRC/share/applications/typingpet.desktop" "$APP_DIR/"
    info "已安装桌面项 $APP_DIR/typingpet.desktop"
fi
command -v update-desktop-database >/dev/null 2>&1 && \
    update-desktop-database "$APP_DIR" 2>/dev/null || true

# 键输入权限
echo
if [ "$WITH_UDEV" = yes ]; then
    if [ "$MODE" = system ] && [ "$(id -u)" = 0 ]; then
        install -m644 "$SRC/share/typingpet/60-typingpet-input.rules" "$UDEV_DEST"
        command -v udevadm >/dev/null 2>&1 && {
            udevadm control --reload 2>/dev/null || true
            udevadm trigger --subsystem-match=input 2>/dev/null || true
        }
        info "已安装键输入权限规则 $UDEV_DEST"
    else
        echo "!! 还差一步：键输入权限（否则监听不到按键）"
        echo "   任选其一："
        echo "     sudo install -m644 \"$SRC/share/typingpet/60-typingpet-input.rules\" $UDEV_DEST"
        echo "     sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=input"
        echo "   或： sudo usermod -aG input \"\$USER\"   # 需重新登录（安全性更低）"
    fi
else
    echo "!! 已按 --no-udev 跳过键输入权限安装；没配过的话程序监听不到按键"
fi

echo
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) echo "提示：$BIN_DIR 不在 PATH 里，可加到 ~/.bashrc：export PATH=\"$BIN_DIR:\$PATH\"" ;;
esac
echo "== 完成 =="
echo "   启动:      $BIN_DIR/typingpet        （右键宠物或再启动一次可打开设置）"
echo "   自检:      $BIN_DIR/typingpet --headless --mock=3"
echo "   按键自检:  $BIN_DIR/typingpet --help 后看 --print-events"
echo "   卸载:      sh $SRC/install.sh --uninstall"

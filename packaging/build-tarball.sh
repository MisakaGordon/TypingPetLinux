#!/bin/sh
# 构建可分发 tarball：
#   bin/typingpet                     主程序
#   lib/libgtk4-layer-shell.so.0      随包附带（系统已有则安装时跳过）
#   share/applications/*.desktop      桌面项
#   share/typingpet/*.rules           键输入权限的 udev 规则
#   install.sh / BUILD-INFO.txt / README.md
#
# 用法: sh packaging/build-tarball.sh [输出目录]
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"

VERSION=$(grep -m1 '^version' Cargo.toml | sed 's/.*= *"\(.*\)"/\1/')
ARCH=$(uname -m)
NAME="typingpet-$VERSION-$ARCH"
OUT=${1:-"$project_dir/dist"}
STAGE="$OUT/$NAME"

echo "==> 构建 release（$NAME）"
cargo build --release -p typingpet

BIN="target/release/typingpet"
[ -x "$BIN" ] || { echo "!! 找不到 $BIN" >&2; exit 1; }

rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/lib" "$STAGE/share/applications" "$STAGE/share/typingpet"

cp "$BIN" "$STAGE/bin/typingpet"
cp packaging/typingpet.desktop        "$STAGE/share/applications/"
cp packaging/60-typingpet-input.rules "$STAGE/share/typingpet/"
cp packaging/install.sh               "$STAGE/install.sh"
chmod +x "$STAGE/install.sh" "$STAGE/bin/typingpet"

# 附带 gtk4-layer-shell：很多发行版没有这个包，缺了程序根本起不来。
# soname 稳定为 .so.0；安装脚本会在系统已有该库时跳过随包那份。
LAYER=$(ldconfig -p 2>/dev/null | awk '/libgtk4-layer-shell\.so\.0/ {print $NF; exit}')
if [ -n "${LAYER:-}" ] && [ -f "$LAYER" ]; then
    cp -L "$LAYER" "$STAGE/lib/libgtk4-layer-shell.so.0"
    echo "==> 已附带 libgtk4-layer-shell.so.0 ($(du -h "$STAGE/lib/libgtk4-layer-shell.so.0" | cut -f1))"
else
    rmdir "$STAGE/lib"
    echo "==> 警告：本机没有 libgtk4-layer-shell.so.0，包内不含该库"
fi

GLIBC_FLOOR=$(readelf --version-info "$BIN" 2>/dev/null \
    | grep -oE 'GLIBC_[0-9]+\.[0-9]+' | sort -uV | tail -1 || echo "unknown")
GTK_NEEDS=$(
    if readelf --dyn-syms --wide "$BIN" 2>/dev/null | grep -q gtk_css_provider_load_from_string; then
        echo "4.12"
    elif readelf --dyn-syms --wide "$BIN" 2>/dev/null | grep -q gtk_file_dialog_new; then
        echo "4.10"
    else
        echo "4.6"
    fi
)

cat > "$STAGE/BUILD-INFO.txt" <<EOF
TypingPet $VERSION ($ARCH)
built:       $(date -u '+%Y-%m-%d %H:%M UTC')
binary:      $(du -h "$STAGE/bin/typingpet" | cut -f1)
glibc floor: $GLIBC_FLOOR   （目标机 glibc 需 >= 此版本）
gtk4 floor:  $GTK_NEEDS     （需要 GTK >= $GTK_NEEDS 的运行库）
bundled:     $( [ -f "$STAGE/lib/libgtk4-layer-shell.so.0" ] && echo "libgtk4-layer-shell.so.0" || echo "无" )
EOF
cat "$STAGE/BUILD-INFO.txt"

cat > "$STAGE/README.md" <<'EOF'
# TypingPet for Linux — 安装包

## 安装

```sh
tar xf typingpet-*.tar.gz
cd typingpet-*/
sh install.sh              # 安装到 ~/.local（普通用户）
sudo sh install.sh --system   # 安装到 /usr/local（推荐：可以顺带装 udev 规则）
sh install.sh --help       # 全部选项
sh install.sh --check      # 只做兼容性检查，不改动系统
```

键输入权限（二选一，装完必须做一次，否则无法监听按键）：

```sh
# 方式 A：随包安装 udev 规则（仅对当前登录用户授权，登出失效）
sudo install -m644 share/typingpet/60-typingpet-input.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=input
# 方式 B：加入 input 组（安全性更低）
sudo usermod -aG input "$USER"   # 需重新登录
```

自检：

```sh
~/.local/bin/typingpet --headless --mock=3   # 不开窗，验证状态机
~/.local/bin/typingpet                       # 启动宠物（托盘/右键宠物可打开设置）
```

卸载：`sh install.sh --uninstall`（配置与图片集保留，如需清除见 `--help`）
EOF

( cd "$OUT" && tar -czf "$NAME.tar.gz" "$NAME" )
if command -v sha256sum >/dev/null 2>&1; then
    ( cd "$OUT" && sha256sum "$NAME.tar.gz" > "$NAME.tar.gz.sha256" )
fi

echo
echo "==> 产物"
ls -lh "$OUT/$NAME.tar.gz" | awk '{print "    " $9 "  " $5}'
[ -f "$OUT/$NAME.tar.gz.sha256" ] && echo "    $OUT/$NAME.tar.gz.sha256"

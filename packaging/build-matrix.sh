#!/bin/sh
# 本地为多个发行版各构建一份产物（manylinux 思路：在较旧的基线里构建，兼容更新的系统）。
#
#   sh packaging/build-matrix.sh                  # 默认目标
#   TARGETS="debian12|debian:12|apt" sh packaging/build-matrix.sh
#   PODMAN=docker sh packaging/build-matrix.sh
#
# 每行格式: 名字|镜像|包管理器家族；镜像写 local 表示直接用本机工具链构建。
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"

RUNTIME=${PODMAN:-}
if [ -z "$RUNTIME" ]; then
    if command -v podman >/dev/null 2>&1; then RUNTIME=podman
    elif command -v docker >/dev/null 2>&1; then RUNTIME=docker
    else echo "需要 podman 或 docker" >&2; exit 1; fi
fi

# docker.io 在很多网络下不可达，默认走可达的镜像源
TARGETS=${TARGETS:-"native|local|
fedora42|registry.fedoraproject.org/fedora:42|dnf|
debian12|docker.m.daocloud.io/library/debian:12|apt|
ubuntu2204|docker.m.daocloud.io/library/ubuntu:22.04|apt|
rocky9|docker.m.daocloud.io/library/rockylinux:9|dnf"}

# 注意：管道里的 while 会 fork 子 shell，失败信息必须走文件才能带出来
failed_file=$(mktemp)
printf '%s\n' "$TARGETS" | while IFS='|' read -r name image family; do
    # set -u 下 read 对字段不足的行不会给变量赋值，先统一兜底再判断
    name=${name:-}; image=${image:-local}; family=${family:-}
    [ -n "$name" ] || continue
    if [ "$image" != local ] && [ -z "$family" ]; then
        echo "!! [$name] 目标格式应为 名字|镜像|包管理器家族" >&2
        echo "$name" >> "$failed_file"
        continue
    fi
    if [ "${image:-local}" = local ]; then
        echo "=================================================================="
        echo "==> [$name] 使用本机工具链"
        TARGET_SUFFIX=$name sh packaging/build-tarball.sh
        continue
    fi
    echo "=================================================================="
    echo "==> [$name] 在容器 $image 内构建（包管理器: $family）"
    # 源码挂进容器，target 目录放在容器内，避免污染宿主与不同工具链互相覆盖
    # SELinux Enforcing 的宿主上，挂载目录默认带 user_home_t，容器读不到 → 用 :Z 私有重标记。
    # 若你的环境不允许重标记，可设 MOUNT_OPTS="--security-opt=label=disable" 或 MOUNT_OPTS=。
    if "$RUNTIME" run --rm \
        -v "$project_dir":/src${MOUNT_OPTS-:Z} \
        -w /src \
        -e CARGO_TARGET_DIR=/tmp/target \
        -e TARGET_SUFFIX="$name" \
        -e APT_MIRROR="${APT_MIRROR:-}" \
        -e DNF_MIRROR="${DNF_MIRROR:-}" \
        "$image" \
        sh packaging/container-build.sh "$family"
    then
        echo "==> [$name] 完成"
    else
        echo "!! [$name] 失败" >&2
        echo "$name" >> "$failed_file"
    fi
done

echo
echo "==> 产物清单"
ls -lh dist/*.tar.gz 2>/dev/null | awk '{print "    " $9 "  " $5}'
if [ -s "$failed_file" ]; then
    echo "!! 失败的目标: $(tr '\n' ' ' < "$failed_file")" >&2
    rm -f "$failed_file"
    exit 1
fi
rm -f "$failed_file"
echo "==> 全部成功"

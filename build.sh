#!/usr/bin/env bash
# 跨平台构建 wxapkg。产物输出到 ./dist
#
# 推荐工具链：rustup + zig（用 cargo-zigbuild 做 Linux / Windows / 跨架构 macOS 交叉编译）
#   macOS:
#   brew install rustup zig
#   Linux:
#   使用发行版包管理器或官方安装脚本安装 rustup 和 zig
#
#   rustup-init -y
#   cargo install cargo-zigbuild
#   rustup target add x86_64-apple-darwin aarch64-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-pc-windows-gnu
#
# Android 目标需要 Android NDK：
#   export ANDROID_NDK_HOME=/path/to/android-ndk
#   rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
#   ANDROID_API=23 ./build.sh
#
# 不愿装 zig 时，Windows 目标也可改用 mingw-w64：
#   brew install mingw-w64
#   rustup target add x86_64-pc-windows-gnu
#   （此时下面的 windows 目标用 cargo build 即可）

set -euo pipefail

cd "$(dirname "$0")"
mkdir -p dist

if command -v cargo-zigbuild >/dev/null 2>&1 && command -v zig >/dev/null 2>&1; then
  BUILD="cargo zigbuild"
else
  BUILD="cargo build"
  echo "提示: 未检测到 cargo-zigbuild + zig，使用 cargo build。"
  echo "      Linux / Windows / 跨架构 macOS 目标需要对应链接器，否则会失败。"
fi

build_target() {
  local target="$1"
  local out_name="$2"
  echo "==> 构建 $target"
  $BUILD --release --target "$target"
  local src="target/$target/release/wxapkg"
  [[ "$target" == *windows* ]] && src="${src}.exe"
  cp "$src" "dist/$out_name"
  echo "    -> dist/$out_name"
}

ndk_host_tag() {
  case "$(uname -s)" in
    Darwin) echo "darwin-x86_64" ;;
    Linux) echo "linux-x86_64" ;;
    MINGW*|MSYS*|CYGWIN*) echo "windows-x86_64" ;;
    *) return 1 ;;
  esac
}

android_linker_path() {
  local ndk="$1"
  local tool="$2"
  local host
  host="$(ndk_host_tag)" || return 1
  printf '%s/toolchains/llvm/prebuilt/%s/bin/%s\n' "$ndk" "$host" "$tool"
}

build_android_target() {
  local target="$1"
  local abi="$2"
  local tool_prefix="$3"
  local api="${ANDROID_API:-23}"
  local ndk="${ANDROID_NDK_HOME:-${NDK_HOME:-}}"

  if [[ -z "$ndk" ]]; then
    echo "跳过 Android $abi: 未设置 ANDROID_NDK_HOME 或 NDK_HOME。"
    return
  fi

  local linker
  if ! linker="$(android_linker_path "$ndk" "${tool_prefix}${api}-clang")"; then
    echo "跳过 Android $abi: 当前构建主机不支持自动定位 NDK linker。"
    return
  fi
  if [[ ! -x "$linker" ]]; then
    echo "跳过 Android $abi: 未找到 NDK linker: $linker"
    return
  fi

  local env_target
  env_target="$(printf '%s' "$target" | tr '[:lower:]-' '[:upper:]_')"
  export "CARGO_TARGET_${env_target}_LINKER=$linker"

  echo "==> 构建 Android $abi ($target, API $api)"
  cargo build --release --target "$target"

  local out_name="wxapkg_android_${abi}"
  cp "target/$target/release/wxapkg" "dist/$out_name"
  echo "    -> dist/$out_name"
}

build_target aarch64-apple-darwin    wxapkg_macos_arm64
build_target x86_64-apple-darwin     wxapkg_macos_amd64
build_target x86_64-unknown-linux-gnu wxapkg_linux_amd64
build_target aarch64-unknown-linux-gnu wxapkg_linux_arm64
build_target x86_64-pc-windows-gnu   wxapkg_windows_amd64.exe

build_android_target aarch64-linux-android     arm64-v8a    aarch64-linux-android
build_android_target armv7-linux-androideabi   armeabi-v7a   armv7a-linux-androideabi
build_android_target x86_64-linux-android      x86_64        x86_64-linux-android

# zigbuild 产出的 Mach-O ad-hoc 签名与 macOS 14+ 不兼容（运行时 SIGKILL）。
# 在 macOS 上用系统 codesign 重签即可绕过；非 macOS 主机会跳过这步。
if [[ "$OSTYPE" == "darwin"* ]] && command -v codesign >/dev/null 2>&1; then
  echo "==> codesign 重签 macOS 二进制"
  codesign --force --sign - dist/wxapkg_macos_arm64 dist/wxapkg_macos_amd64
fi

echo
echo "构建完成："
ls -lh dist/

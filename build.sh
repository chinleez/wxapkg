#!/usr/bin/env bash
# 跨平台构建 wxapkg。产物输出到 ./dist
#
# 推荐工具链：rustup + zig（用 cargo-zigbuild 做 Windows / 跨架构 macOS 交叉编译）
#   brew install rustup zig
#   rustup-init -y
#   cargo install cargo-zigbuild
#   rustup target add x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-gnu
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
  echo "      Windows / 跨架构 macOS 目标需要对应链接器，否则会失败。"
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

build_target aarch64-apple-darwin    wxapkg_macos_arm64
build_target x86_64-apple-darwin     wxapkg_macos_amd64
build_target x86_64-pc-windows-gnu   wxapkg_windows_amd64.exe

# zigbuild 产出的 Mach-O ad-hoc 签名与 macOS 14+ 不兼容（运行时 SIGKILL）。
# 在 macOS 上用系统 codesign 重签即可绕过；非 macOS 主机会跳过这步。
if [[ "$OSTYPE" == "darwin"* ]] && command -v codesign >/dev/null 2>&1; then
  echo "==> codesign 重签 macOS 二进制"
  codesign --force --sign - dist/wxapkg_macos_arm64 dist/wxapkg_macos_amd64
fi

echo
echo "构建完成："
ls -lh dist/

# wxapkg

微信小程序 `.wxapkg` 解密和解包工具，Rust 实现，支持加密 / 未加密包自动识别。

> 仅用于分析自己有权限处理的小程序包。

## 下载

在 [Releases](https://github.com/chinleez/wxapkg/releases/) 页面下载对应平台的二进制：

| 平台 | 文件 |
| --- | --- |
| Windows x64 | `wxapkg_windows_amd64.exe` |
| macOS Apple Silicon | `wxapkg_macos_arm64` |
| macOS Intel | `wxapkg_macos_amd64` |
| Android arm64-v8a | `wxapkg_android_arm64-v8a` |
| Android armeabi-v7a | `wxapkg_android_armeabi-v7a` |
| Android x86_64 | `wxapkg_android_x86_64` |

大多数 Android 真机使用 `arm64-v8a`；Android 模拟器可能使用 `x86_64`。

## 使用方法

```bash
wxapkg <path-to-wxapkg> [-w <wxid>]
```

工具会自动判断文件是否加密：

- 未加密包：直接解包到同目录的 `<原文件名>_unpack/`
- 加密包：先生成 `<原文件名>_decrypt`，再解包到 `<原文件名>_decrypt_unpack/`
- `-w <wxid>` 仅加密包需要；如果路径包含 `.../packages/{wxid}/...`，通常可以省略

### Windows

可以在命令行运行：

```powershell
.\wxapkg_windows_amd64.exe C:\path\to\__APP__.wxapkg
```

也可以直接把 `.wxapkg` 文件拖到 exe 上运行。

### macOS

下载后先添加执行权限：

```bash
chmod +x ./wxapkg_macos_arm64
./wxapkg_macos_arm64 /path/to/__APP__.wxapkg
```

Intel Mac 使用 `wxapkg_macos_amd64`。如果 macOS 阻止运行，请在系统安全设置中允许该二进制，或在终端里重新执行。

### Android

先把二进制和 `.wxapkg` 文件推到设备可访问目录：

```bash
adb push wxapkg_android_arm64-v8a /data/local/tmp/wxapkg
adb push __APP__.wxapkg /data/local/tmp/__APP__.wxapkg
adb shell chmod +x /data/local/tmp/wxapkg
adb shell /data/local/tmp/wxapkg /data/local/tmp/__APP__.wxapkg
```

如果处理加密包且工具无法从路径推断 `wxid`，手动指定：

```bash
adb shell /data/local/tmp/wxapkg /data/local/tmp/__APP__.wxapkg -w wx1234567890abcdef
```

Android 应用私有目录通常需要 root、备份导出或其他方式才能取得原始 `.wxapkg` 文件。

## 找到 wxapkg 文件

不带参数运行二进制会显示当前平台的常见路径。常见位置可能包括：

- **Windows (微信 4.0+)**：`%AppData%\Tencent\xwechat\radium\Applet\packages\{wxid}\{n}\__APP__.wxapkg`
- **macOS**：`~/Library/Containers/com.tencent.xinWeChat/Data/.wxapplet/packages/{wxid}/{n}/__APP__.wxapkg`

不同微信版本或缓存状态下路径可能不同，可以在微信数据目录里搜索 `__APP__.wxapkg`。

## 从源码构建

本机调试：

```bash
cargo build --release
```

跨平台构建脚本会把产物输出到 `dist/`：

```bash
./build.sh
```

推荐工具链：

```bash
brew install rustup zig
rustup-init -y
cargo install cargo-zigbuild
rustup target add x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-gnu
```

构建 Android 目标需要 Android NDK：

```bash
export ANDROID_NDK_HOME=/path/to/android-ndk
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
ANDROID_API=23 ./build.sh
```

`ANDROID_API` 默认是 `23`。

## 注意事项

- 目前 Release 不提供 Linux 预编译二进制；Linux 用户可以从源码构建。
- 解包输出会覆盖同名目标文件，请在需要时先备份已有输出目录。
- 工具会校验包内文件范围和单文件大小，尽量避免畸形包导致异常内存占用。

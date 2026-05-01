# wxapkg

🚀 微信小程序一键解密和解包工具（Rust 实现）

## 📥 下载

在 [Release](https://github.com/chinleez/wxapkg/releases/) 页面下载对应平台的二进制：

- **Windows**: `wxapkg_windows_amd64.exe`
- **macOS (Apple Silicon)**: `wxapkg_macos_arm64`
- **macOS (Intel)**: `wxapkg_macos_amd64`

## 🚀 使用方法

```
wxapkg <path-to-wxapkg> [-w <wxid>]
```

加密 / 未加密自动识别，结果输出到同目录的 `<原文件名>_unpack/`。`-w` 仅加密文件需要，路径里有 `.../packages/{wxid}/...` 时可省略。

**Windows**：拖拽 `.wxapkg` 到 exe 上即可。

![演示GIF](https://github.com/zhuweiyou/wxapkg/assets/8413791/07a5cfa5-00c9-47b5-aaa3-ee42b878495f)

### 找到 wxapkg 文件

不带参数运行二进制即可看到当前用户的默认路径。常见位置：

- **Windows (微信 4.0+)**：`%AppData%\Tencent\xwechat\radium\Applet\packages\{wxid}\{n}\__APP__.wxapkg`
- **macOS**：`~/Library/Containers/com.tencent.xinWeChat/Data/.wxapplet/packages/{wxid}/{n}/__APP__.wxapkg`

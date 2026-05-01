use std::env;
use std::fs::File;
use std::io::Read;
use std::process::ExitCode;

mod decrypter;
mod unpacker;

const WXAPKG_MAGIC: u8 = 0xBE;
const PROG: &str = "wxapkg";

fn main() -> ExitCode {
    let outcome = run();

    match &outcome {
        Ok(Outcome::Success) => println!("success"),
        Ok(Outcome::Usage) => {}
        Err(msg) => eprintln!("error: {}", msg),
    }

    pause_if_dragdrop();

    match outcome {
        Ok(_) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

enum Outcome {
    Success,
    Usage,
}

fn run() -> Result<Outcome, String> {
    let Some(args) = parse_args() else {
        print_usage();
        return Ok(Outcome::Usage);
    };

    let mut from = decrypter::format_from(&args.path);
    println!("from {}", from);

    if needs_decrypt(&from)? {
        let wxid = args
            .wxid
            .or_else(|| decrypter::get_wxid(&from))
            .ok_or_else(|| {
                "文件已加密，但无法识别 wxid。请用 -w <wxid> 指定，\
                 或保留原始 .../packages/{wxid}/{n}/__APP__.wxapkg 路径运行"
                    .to_string()
            })?;

        println!("wxid {}", wxid);
        decrypter::default_decrypt(&from, &wxid)?;
        from.push_str(decrypter::DEFAULT_DECRYPT_TO);
    }

    unpacker::unpack(&from)?;
    Ok(Outcome::Success)
}

fn needs_decrypt(path: &str) -> Result<bool, String> {
    let mut f = File::open(path).map_err(|e| format!("打开 {} 失败: {}", path, e))?;
    let mut head = [0u8; 1];
    f.read_exact(&mut head)
        .map_err(|e| format!("读取 {} 失败: {}", path, e))?;
    Ok(head[0] != WXAPKG_MAGIC)
}

struct Args {
    path: String,
    wxid: Option<String>,
}

fn parse_args() -> Option<Args> {
    let mut iter = env::args().skip(1);
    let mut path: Option<String> = None;
    let mut wxid: Option<String> = None;

    while let Some(a) = iter.next() {
        match a.as_str() {
            "-w" | "--wxid" => wxid = iter.next(),
            "-h" | "--help" => return None,
            _ if path.is_none() => path = Some(a),
            _ => return None,
        }
    }

    path.map(|p| Args { path: p, wxid })
}

fn print_usage() {
    println!("用法: {} <path> [-w <wxid>]", PROG);
    println!();
    println!("加密 / 未加密自动识别。-w 仅加密文件需要，路径含 .../packages/{{wxid}}/... 时可省略。");
    println!();
    println!("常见路径:");
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "~".to_string());
        println!("  {}/Library/Containers/com.tencent.xinWeChat/Data/.wxapplet/packages/{{wxid}}/{{n}}/__APP__.wxapkg", home);
    }
    #[cfg(target_os = "windows")]
    {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| r"C:\Users\<用户名>".to_string());
        println!(r"  {}\AppData\Roaming\Tencent\xwechat\radium\Applet\packages\{{wxid}}\{{n}}\__APP__.wxapkg", home);
    }
}

/// Windows 用户常通过拖拽文件到 exe 上运行，控制台会立即关闭看不到输出。
/// 故仅在 Windows 平台保留按键退出；终端环境下使用不会被阻塞。
#[cfg(target_os = "windows")]
fn pause_if_dragdrop() {
    let mut buf = [0u8; 1];
    let _ = std::io::stdin().read(&mut buf);
}

#[cfg(not(target_os = "windows"))]
fn pause_if_dragdrop() {}

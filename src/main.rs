use std::env;
use std::fs;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod decrypter;
mod logging;
mod unpacker;

const WXAPKG_MAGIC: u8 = 0xBE;
const PROG: &str = "wxapkg";

fn main() -> ExitCode {
    let outcome = run();

    match &outcome {
        Ok(Outcome::Success) => {
            if logging::is_normal() {
                println!("success");
            }
        }
        Ok(Outcome::Usage) => {}
        Err(msg) => {
            eprintln!("error: {}", msg);
            eprintln!("提示: 使用 --help 查看用法");
        }
    }

    pause_if_dragdrop();

    match outcome {
        Ok(_) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Success,
    Usage,
}

fn run() -> Result<Outcome, String> {
    run_with_args(env::args().skip(1))
}

fn run_with_args<I, S>(raw_args: I) -> Result<Outcome, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    logging::set(logging::LogLevel::Normal);

    let args = match parse_args_from(raw_args)? {
        ParseArgs::Help => {
            print_usage();
            return Ok(Outcome::Usage);
        }
        ParseArgs::Run(args) => args,
    };

    logging::set(args.log_level);

    let input_path = Path::new(&args.path);
    if input_path.is_file() {
        process_wxapkg_file(input_path, args.wxid.as_deref())?;
        return Ok(Outcome::Success);
    }
    if input_path.is_dir() {
        process_wxapkg_dir(input_path, args.wxid.as_deref())?;
        return Ok(Outcome::Success);
    }

    Err(format!(
        "路径不存在或不可处理: {}（仅支持文件或目录）",
        args.path
    ))
}

fn process_wxapkg_dir(dir: &Path, wxid_arg: Option<&str>) -> Result<(), String> {
    let files = collect_wxapkg_files(dir)?;
    if files.is_empty() {
        return Err(format!("目录 {} 下未找到 .wxapkg 文件", dir.display()));
    }

    logging::info(format!(
        "目录 {} 下找到 {} 个 .wxapkg 文件",
        dir.display(),
        files.len()
    ));

    let total = files.len();
    let mut failures = Vec::new();

    for path in files {
        if let Err(err) = process_wxapkg_file(&path, wxid_arg) {
            failures.push((path, err));
        }
    }

    if failures.is_empty() {
        logging::info(format!("批量处理完成: 成功 {}，失败 0", total));
        return Ok(());
    }

    let mut msg = format!(
        "批量处理完成: 成功 {}，失败 {}",
        total - failures.len(),
        failures.len()
    );
    for (idx, (path, err)) in failures.iter().take(5).enumerate() {
        msg.push_str(&format!("\n{}. {}: {}", idx + 1, path.display(), err));
    }
    if failures.len() > 5 {
        msg.push_str(&format!(
            "\n... 其余 {} 个失败请开启 --verbose 逐个排查",
            failures.len() - 5
        ));
    }
    Err(msg)
}

fn collect_wxapkg_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries =
            fs::read_dir(&dir).map_err(|e| format!("读取目录 {} 失败: {}", dir.display(), e))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("读取目录项 {} 失败: {}", dir.display(), e))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|e| format!("读取文件类型 {} 失败: {}", path.display(), e))?;

            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if file_type.is_file() && is_wxapkg_file(&path) {
                files.push(path);
            }
        }
    }

    files.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
    Ok(files)
}

fn is_wxapkg_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("wxapkg"))
        .unwrap_or(false)
}

fn process_wxapkg_file(path: &Path, wxid_arg: Option<&str>) -> Result<(), String> {
    let mut raw_from = path
        .to_str()
        .ok_or_else(|| format!("路径 {} 包含无法解析字符", path.display()))?
        .to_string();
    let normalized_from = decrypter::format_from(&raw_from);
    logging::info(format!("from {}", normalized_from));

    if needs_decrypt(&raw_from)? {
        let wxid = wxid_arg
            .map(str::to_string)
            .or_else(|| decrypter::get_wxid(&normalized_from))
            .ok_or_else(|| {
                format!(
                    "{} 已加密，但无法识别 wxid。请用 -w <wxid> 指定，或保留原始 .../packages/{{wxid}}/{{n}}/__APP__.wxapkg 路径运行",
                    path.display()
                )
            })?;

        logging::info(format!("wxid {}", wxid));
        decrypter::default_decrypt(&raw_from, &wxid)?;
        raw_from.push_str(decrypter::DEFAULT_DECRYPT_TO);
    }

    unpacker::unpack(&raw_from)?;
    Ok(())
}

fn needs_decrypt(path: &str) -> Result<bool, String> {
    let mut f = File::open(path).map_err(|e| format!("打开 {} 失败: {}", path, e))?;
    let mut head = [0u8; 1];
    f.read_exact(&mut head)
        .map_err(|e| format!("读取 {} 失败: {}", path, e))?;
    Ok(head[0] != WXAPKG_MAGIC)
}

#[derive(Debug)]
struct Args {
    path: String,
    wxid: Option<String>,
    log_level: logging::LogLevel,
}

#[derive(Debug)]
enum ParseArgs {
    Help,
    Run(Args),
}

fn parse_args_from<I, S>(raw_args: I) -> Result<ParseArgs, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut iter = raw_args.into_iter().map(Into::into).peekable();
    let mut path: Option<String> = None;
    let mut wxid: Option<String> = None;
    let mut quiet = false;
    let mut verbose = false;

    while let Some(a) = iter.next() {
        match a.as_str() {
            "-w" | "--wxid" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "`-w/--wxid` 缺少参数".to_string())?;
                wxid = Some(value);
            }
            "-q" | "--quiet" => quiet = true,
            "-v" | "--verbose" => verbose = true,
            "-h" | "--help" => return Ok(ParseArgs::Help),
            "--" => {
                let trailing_path = iter
                    .next()
                    .ok_or_else(|| "`--` 后缺少 <path> 参数".to_string())?;
                if path.is_some() {
                    return Err("参数过多，仅支持一个 <path>".to_string());
                }
                path = Some(trailing_path);
                if iter.next().is_some() {
                    return Err("参数过多，仅支持一个 <path>".to_string());
                }
                break;
            }
            _ if a.starts_with('-') => return Err(format!("未知参数: {}", a)),
            _ if path.is_none() => path = Some(a),
            _ => return Err("参数过多，仅支持一个 <path>".to_string()),
        }
    }

    if quiet && verbose {
        return Err("`--quiet` 与 `--verbose` 不能同时使用".to_string());
    }

    let path = path.ok_or_else(|| "缺少 <path> 参数".to_string())?;
    let log_level = if quiet {
        logging::LogLevel::Quiet
    } else if verbose {
        logging::LogLevel::Verbose
    } else {
        logging::LogLevel::Normal
    };

    Ok(ParseArgs::Run(Args {
        path,
        wxid,
        log_level,
    }))
}

fn print_usage() {
    println!("用法: {} <path> [选项]", PROG);
    println!();
    println!(
        "加密 / 未加密自动识别。-w 仅加密文件需要，路径含 .../packages/{{wxid}}/... 时可省略。"
    );
    println!("path 可为单个 .wxapkg 文件，或目录（会递归查找目录下所有 .wxapkg）。");
    println!();
    println!("选项:");
    println!("  -w, --wxid <wxid>   手动指定 wxid");
    println!("  -q, --quiet         仅输出错误");
    println!("  -v, --verbose       输出详细过程");
    println!("  -h, --help          显示帮助");
    println!();
    println!("常见路径:");
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "~".to_string());
        println!("  {}/Library/Containers/com.tencent.xinWeChat/Data/.wxapplet/packages/{{wxid}}/{{n}}/__APP__.wxapkg", home);
    }
    #[cfg(target_os = "windows")]
    {
        let home =
            std::env::var("USERPROFILE").unwrap_or_else(|_| r"C:\Users\<用户名>".to_string());
        println!(
            r"  {}\AppData\Roaming\Tencent\xwechat\radium\Applet\packages\{{wxid}}\{{n}}\__APP__.wxapkg",
            home
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        dir.push(format!("{}_{}_{}", prefix, std::process::id(), nanos));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn help_flag_returns_usage() {
        let outcome = run_with_args(vec!["--help"]);
        assert_eq!(outcome, Ok(Outcome::Usage));
    }

    #[test]
    fn missing_path_returns_error() {
        let err = run_with_args(Vec::<String>::new()).unwrap_err();
        assert!(err.contains("缺少 <path> 参数"));
    }

    #[test]
    fn unknown_flag_returns_error() {
        let err = parse_args_from(vec!["--bad"]).unwrap_err();
        assert!(err.contains("未知参数"));
    }

    #[test]
    fn wxid_requires_value() {
        let err = parse_args_from(vec!["/tmp/a.wxapkg", "-w"]).unwrap_err();
        assert!(err.contains("缺少参数"));
    }

    #[test]
    fn quiet_and_verbose_conflict() {
        let err = parse_args_from(vec!["/tmp/a.wxapkg", "--quiet", "--verbose"]).unwrap_err();
        assert!(err.contains("不能同时使用"));
    }

    #[test]
    fn parse_valid_args_with_quiet() {
        let parsed = parse_args_from(vec!["/tmp/a.wxapkg", "-w", "wx123", "--quiet"]).unwrap();
        let ParseArgs::Run(args) = parsed else {
            panic!("expected run args");
        };
        assert_eq!(args.path, "/tmp/a.wxapkg");
        assert_eq!(args.wxid.as_deref(), Some("wx123"));
        assert_eq!(args.log_level, logging::LogLevel::Quiet);
    }

    #[test]
    fn directory_mode_unpacks_all_wxapkg_files() {
        let sample = Path::new("res/sample.wxapkg");
        assert!(sample.exists(), "missing res/sample.wxapkg");

        let dir = temp_dir("wxapkg_dir_mode");
        let nested = dir.join("nested");
        fs::create_dir_all(&nested).expect("create nested dir");
        fs::copy(sample, dir.join("a.wxapkg")).expect("copy sample a");
        fs::copy(sample, nested.join("b.wxapkg")).expect("copy sample b");

        let args = vec![dir.to_string_lossy().into_owned(), "--quiet".to_string()];
        run_with_args(args).expect("directory mode should succeed");

        assert!(dir.join("a.wxapkg_unpack/app-config.json").exists());
        assert!(nested.join("b.wxapkg_unpack/app-config.json").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn directory_mode_reports_when_no_wxapkg() {
        let dir = temp_dir("wxapkg_dir_empty");
        fs::write(dir.join("note.txt"), b"noop").expect("write note");

        let args = vec![dir.to_string_lossy().into_owned(), "--quiet".to_string()];
        let err = run_with_args(args).unwrap_err();
        assert!(err.contains("未找到 .wxapkg"));

        let _ = fs::remove_dir_all(&dir);
    }
}

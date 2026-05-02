use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

pub const DEFAULT_UNPACK_TO: &str = "_unpack";

const HEADER_FIRST: u8 = 0xBE;
const HEADER_LAST: u8 = 0xED;
/// 上限保护：单个 wxapkg 不可能有这么多文件，超过即视为格式损坏，避免 OOM。
const MAX_FILE_COUNT: u32 = 1_000_000;
/// 单个包内文件的最大解包大小，避免畸形包触发超大内存分配。
const MAX_ENTRY_SIZE: u64 = 512 * 1024 * 1024;

struct Entry {
    name: String,
    offset: u32,
    size: u32,
}

pub fn unpack(from: &str) -> Result<(), String> {
    let f = File::open(from).map_err(|e| format!("打开 {} 失败: {}", from, e))?;
    let file_len = f
        .metadata()
        .map_err(|e| format!("读取 {} 元数据失败: {}", from, e))?
        .len();
    let mut reader = BufReader::new(f);

    let path = Path::new(from);
    let root = path.parent().unwrap_or_else(|| Path::new(""));
    let base = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "无法解析输入文件名".to_string())?;
    let dest = root.join(format!("{}{}", base, DEFAULT_UNPACK_TO));

    let entries = read_header(&mut reader)?;
    println!("fileCount = {}", entries.len());

    for entry in &entries {
        validate_entry_bounds(entry, file_len)?;

        let target = resolve_safe_path(&dest, &entry.name)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("创建目录 {} 失败: {}", parent.display(), e))?;
        }

        reader
            .seek(SeekFrom::Start(entry.offset as u64))
            .map_err(|e| format!("seek 到偏移 {} 失败: {}", entry.offset, e))?;

        let mut buf = vec![0u8; entry.size as usize];
        reader
            .read_exact(&mut buf)
            .map_err(|e| format!("读取 {} 内容失败: {}", entry.name, e))?;

        let out = File::create(&target)
            .map_err(|e| format!("创建文件 {} 失败: {}", target.display(), e))?;
        BufWriter::new(out)
            .write_all(&buf)
            .map_err(|e| format!("写入 {} 失败: {}", target.display(), e))?;

        println!("writeFile = {}", target.display());
    }

    Ok(())
}

fn validate_entry_bounds(entry: &Entry, file_len: u64) -> Result<(), String> {
    let offset = entry.offset as u64;
    let size = entry.size as u64;

    if size > MAX_ENTRY_SIZE {
        return Err(format!(
            "{} 大小 {} 超出单文件上限 {}，文件可能损坏",
            entry.name, size, MAX_ENTRY_SIZE
        ));
    }

    let end = offset
        .checked_add(size)
        .ok_or_else(|| format!("{} 偏移和大小溢出，文件可能损坏", entry.name))?;
    if end > file_len {
        return Err(format!(
            "{} 范围 [{}..{}) 超出文件长度 {}，文件可能损坏",
            entry.name, offset, end, file_len
        ));
    }

    Ok(())
}

fn read_header(r: &mut impl Read) -> Result<Vec<Entry>, String> {
    let first = read_u8(r, "first header mark")?;
    println!("first header mark = {}", first);

    let info1 = read_u32_be(r, "info1")?;
    println!("info1 = {}", info1);

    let index_info_length = read_u32_be(r, "indexInfoLength")?;
    println!("indexInfoLength = {}", index_info_length);

    let body_info_length = read_u32_be(r, "bodyInfoLength")?;
    println!("bodyInfoLength = {}", body_info_length);

    let last = read_u8(r, "last header mark")?;
    println!("last header mark = {}", last);

    if first != HEADER_FIRST || last != HEADER_LAST {
        return Err("不是有效的 wxapkg 文件（header magic 不匹配）".to_string());
    }

    let file_count = read_u32_be(r, "fileCount")?;
    if file_count > MAX_FILE_COUNT {
        return Err(format!(
            "fileCount={} 超出上限 {}，文件可能损坏",
            file_count, MAX_FILE_COUNT
        ));
    }

    let mut entries = Vec::with_capacity(file_count as usize);
    for _ in 0..file_count {
        let name_len = read_u32_be(r, "name length")?;
        let mut name_bytes = vec![0u8; name_len as usize];
        r.read_exact(&mut name_bytes)
            .map_err(|e| format!("读取文件名失败: {}", e))?;
        let name = String::from_utf8_lossy(&name_bytes).into_owned();
        let offset = read_u32_be(r, "offset")?;
        let size = read_u32_be(r, "size")?;
        println!("readFile = {} at Offset = {}", name, offset);
        entries.push(Entry { name, offset, size });
    }
    Ok(entries)
}

/// 把 wxapkg 内的相对路径拼到目标目录，并阻止 `..` 路径穿越。
fn resolve_safe_path(dest_root: &Path, raw: &str) -> Result<PathBuf, String> {
    let rel = raw.trim_start_matches('/');
    let rel_path = Path::new(rel);

    for c in rel_path.components() {
        match c {
            Component::Normal(_) => {}
            Component::CurDir => {}
            _ => {
                return Err(format!(
                    "wxapkg 内含可疑路径 {:?}，已拒绝（防止路径穿越）",
                    raw
                ));
            }
        }
    }

    Ok(dest_root.join(rel_path))
}

fn read_u8(r: &mut impl Read, what: &str) -> Result<u8, String> {
    let mut buf = [0u8; 1];
    r.read_exact(&mut buf)
        .map_err(|e| format!("读取 {} 失败: {}", what, e))?;
    Ok(buf[0])
}

fn read_u32_be(r: &mut impl Read, what: &str) -> Result<u32, String> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)
        .map_err(|e| format!("读取 {} 失败: {}", what, e))?;
    Ok(u32::from_be_bytes(buf))
}

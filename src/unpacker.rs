use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

use crate::logging;

pub const DEFAULT_UNPACK_TO: &str = "_unpack";

const HEADER_FIRST: u8 = 0xBE;
const HEADER_LAST: u8 = 0xED;
const HEADER_PREFIX_LEN: u64 = 1 + 4 + 4 + 4 + 1;
const INDEX_MIN_LEN: u64 = 4; // file_count
const ENTRY_FIXED_LEN: u64 = 4 + 4 + 4; // name_len + offset + size
/// 上限保护：单个 wxapkg 不可能有这么多文件，超过即视为格式损坏，避免 OOM。
const MAX_FILE_COUNT: u32 = 1_000_000;
/// 单个包内文件的最大解包大小，避免畸形包触发超大内存分配。
const MAX_ENTRY_SIZE: u64 = 512 * 1024 * 1024;
/// 文件名上限保护，避免畸形 header 触发超大分配。
const MAX_NAME_LEN: u32 = 16 * 1024;

struct Entry {
    name: String,
    offset: u32,
    size: u32,
}

struct Header {
    entries: Vec<Entry>,
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

    let header = read_header(&mut reader, file_len)?;
    logging::info(format!("fileCount = {}", header.entries.len()));

    for entry in &header.entries {
        validate_entry_bounds(entry, file_len)?;

        let target = resolve_safe_path(&dest, &entry.name)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("创建目录 {} 失败: {}", parent.display(), e))?;
        }

        reader
            .seek(SeekFrom::Start(entry.offset as u64))
            .map_err(|e| format!("seek 到偏移 {} 失败: {}", entry.offset, e))?;

        let out = File::create(&target)
            .map_err(|e| format!("创建文件 {} 失败: {}", target.display(), e))?;
        let mut writer = BufWriter::new(out);
        let mut limited = reader.by_ref().take(entry.size as u64);
        let copied = std::io::copy(&mut limited, &mut writer)
            .map_err(|e| format!("读取 {} 内容失败: {}", entry.name, e))?;
        if copied != entry.size as u64 {
            return Err(format!(
                "读取 {} 内容失败: 期望 {} 字节，实际 {} 字节",
                entry.name, entry.size, copied
            ));
        }
        writer
            .flush()
            .map_err(|e| format!("写入 {} 失败: {}", target.display(), e))?;

        logging::verbose(format!("writeFile = {}", target.display()));
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

fn read_header(r: &mut impl Read, file_len: u64) -> Result<Header, String> {
    let first = read_u8(r, "first header mark")?;
    logging::verbose(format!("first header mark = {}", first));

    let info1 = read_u32_be(r, "info1")?;
    logging::verbose(format!("info1 = {}", info1));

    let index_info_length = read_u32_be(r, "indexInfoLength")?;
    logging::verbose(format!("indexInfoLength = {}", index_info_length));

    let body_info_length = read_u32_be(r, "bodyInfoLength")?;
    logging::verbose(format!("bodyInfoLength = {}", body_info_length));

    let last = read_u8(r, "last header mark")?;
    logging::verbose(format!("last header mark = {}", last));

    if first != HEADER_FIRST || last != HEADER_LAST {
        return Err("不是有效的 wxapkg 文件（header magic 不匹配）".to_string());
    }

    validate_file_layout(file_len, index_info_length as u64, body_info_length as u64)?;
    if (index_info_length as u64) < INDEX_MIN_LEN {
        return Err(format!(
            "indexInfoLength={} 小于最小值 {}，文件可能损坏",
            index_info_length, INDEX_MIN_LEN
        ));
    }

    let file_count = read_u32_be(r, "fileCount")?;
    if file_count > MAX_FILE_COUNT {
        return Err(format!(
            "fileCount={} 超出上限 {}，文件可能损坏",
            file_count, MAX_FILE_COUNT
        ));
    }

    let mut index_bytes_used = INDEX_MIN_LEN;
    let index_cap = index_info_length as u64;
    let mut entries = Vec::with_capacity(file_count as usize);
    for _ in 0..file_count {
        let name_len = read_u32_be(r, "name length")?;
        if name_len > MAX_NAME_LEN {
            return Err(format!(
                "文件名长度 {} 超出上限 {}，文件可能损坏",
                name_len, MAX_NAME_LEN
            ));
        }
        let entry_bytes = ENTRY_FIXED_LEN
            .checked_add(name_len as u64)
            .ok_or_else(|| "索引区长度溢出，文件可能损坏".to_string())?;
        index_bytes_used = index_bytes_used
            .checked_add(entry_bytes)
            .ok_or_else(|| "索引区长度溢出，文件可能损坏".to_string())?;
        if index_bytes_used > index_cap {
            return Err(format!(
                "索引区超长: 已读取 {} 字节，声明上限 {} 字节，文件可能损坏",
                index_bytes_used, index_cap
            ));
        }

        let mut name_bytes = vec![0u8; name_len as usize];
        r.read_exact(&mut name_bytes)
            .map_err(|e| format!("读取文件名失败: {}", e))?;
        let name = String::from_utf8_lossy(&name_bytes).into_owned();
        let offset = read_u32_be(r, "offset")?;
        let size = read_u32_be(r, "size")?;
        logging::verbose(format!("readFile = {} at Offset = {}", name, offset));
        entries.push(Entry { name, offset, size });
    }
    if index_bytes_used != index_cap {
        return Err(format!(
            "索引区长度不匹配: 实际读取 {} 字节，header 声明 {} 字节",
            index_bytes_used, index_cap
        ));
    }

    Ok(Header { entries })
}

fn validate_file_layout(
    file_len: u64,
    index_info_length: u64,
    body_info_length: u64,
) -> Result<(), String> {
    let expected = HEADER_PREFIX_LEN
        .checked_add(index_info_length)
        .and_then(|v| v.checked_add(body_info_length))
        .ok_or_else(|| "header 长度溢出，文件可能损坏".to_string())?;
    if expected != file_len {
        return Err(format!(
            "header 长度不一致: header 计算 {} 字节，实际文件 {} 字节",
            expected, file_len
        ));
    }
    Ok(())
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

    fn push_u32_be(buf: &mut Vec<u8>, n: u32) {
        buf.extend_from_slice(&n.to_be_bytes());
    }

    #[test]
    fn rejects_path_traversal() {
        let err = resolve_safe_path(Path::new("/tmp/wxapkg-out"), "../evil.js").unwrap_err();
        assert!(err.contains("可疑路径"));
    }

    #[test]
    fn rejects_oversized_name_length() {
        let dir = temp_dir("wxapkg_bad_name");
        let file = dir.join("bad.wxapkg");

        let mut data = Vec::new();
        data.push(HEADER_FIRST);
        push_u32_be(&mut data, 0);
        push_u32_be(&mut data, (INDEX_MIN_LEN + 4) as u32);
        push_u32_be(&mut data, 0);
        data.push(HEADER_LAST);
        push_u32_be(&mut data, 1);
        push_u32_be(&mut data, MAX_NAME_LEN + 1);

        fs::write(&file, data).expect("write bad wxapkg");
        let err = unpack(file.to_str().expect("utf8 path")).unwrap_err();
        assert!(err.contains("文件名长度"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_header_length_mismatch() {
        let dir = temp_dir("wxapkg_bad_layout");
        let file = dir.join("bad_layout.wxapkg");

        let mut data = Vec::new();
        data.push(HEADER_FIRST);
        push_u32_be(&mut data, 0);
        push_u32_be(&mut data, INDEX_MIN_LEN as u32);
        push_u32_be(&mut data, 0);
        data.push(HEADER_LAST);
        push_u32_be(&mut data, 0);
        data.push(0); // 多余字节，故意制造长度不一致

        fs::write(&file, data).expect("write bad layout wxapkg");
        let err = unpack(file.to_str().expect("utf8 path")).unwrap_err();
        assert!(err.contains("header 长度不一致"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unpacks_repository_sample() {
        let sample = Path::new("res/sample.wxapkg");
        assert!(sample.exists(), "missing res/sample.wxapkg");

        let dir = temp_dir("wxapkg_sample_unpack");
        let input = dir.join("sample.wxapkg");
        fs::copy(sample, &input).expect("copy sample");

        logging::set(logging::LogLevel::Quiet);
        unpack(input.to_str().expect("utf8 path")).expect("unpack sample");

        assert!(dir.join("sample.wxapkg_unpack/app-config.json").exists());
        assert!(dir.join("sample.wxapkg_unpack/app-service.js").exists());

        let _ = fs::remove_dir_all(&dir);
    }
}

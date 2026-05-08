use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};

use aes::cipher::{generic_array::GenericArray, BlockDecryptMut, KeyIvInit};
use aes::Aes256;
use pbkdf2::pbkdf2_hmac_array;
use sha1::Sha1;

pub const DEFAULT_DECRYPT_TO: &str = "_decrypt";

const HEADER_OFFSET: usize = 6;
const ENCRYPTED_BLOCK_LEN: usize = 1024;
const KEEP_LEN: usize = 1023;
const PBKDF2_ROUNDS: u32 = 1000;
const DEFAULT_IV: &[u8; 16] = b"the iv: 16 bytes";
const DEFAULT_SALT: &[u8] = b"saltiest";
const DEFAULT_XOR_KEY: u8 = 0x66;

type Aes256CbcDec = cbc::Decryptor<Aes256>;

pub fn format_from(from: &str) -> String {
    from.replace('\\', "/")
}

/// 从形如 `.../packages/{wxid}/{n}/__APP__.wxapkg` 的路径中推断 wxid。
/// wxid 必须以 "wx" 开头才视为有效。
pub fn get_wxid(from: &str) -> Option<String> {
    let parts: Vec<&str> = from.split('/').collect();
    if parts.len() < 3 {
        return None;
    }
    let candidate = parts[parts.len() - 3];
    candidate.starts_with("wx").then(|| candidate.to_string())
}

pub fn default_decrypt(wxapkg_path: &str, wxid: &str) -> Result<(), String> {
    let dec_path = format!("{}{}", wxapkg_path, DEFAULT_DECRYPT_TO);
    decrypt(wxapkg_path, &dec_path, wxid, DEFAULT_IV, DEFAULT_SALT)
}

fn decrypt(
    wxapkg_path: &str,
    dec_path: &str,
    wxid: &str,
    iv: &[u8; 16],
    salt: &[u8],
) -> Result<(), String> {
    let input = File::open(wxapkg_path).map_err(|e| format!("打开 {} 失败: {}", wxapkg_path, e))?;
    let input_len = input
        .metadata()
        .map_err(|e| format!("读取 {} 元数据失败: {}", wxapkg_path, e))?
        .len();
    if input_len < (HEADER_OFFSET + ENCRYPTED_BLOCK_LEN) as u64 {
        return Err("文件过小，不是有效的 wxapkg".to_string());
    }
    let mut reader = BufReader::new(input);

    let key = pbkdf2_hmac_array::<Sha1, 32>(wxid.as_bytes(), salt, PBKDF2_ROUNDS);

    let mut skipped = [0u8; HEADER_OFFSET];
    reader
        .read_exact(&mut skipped)
        .map_err(|e| format!("读取 {} 头部失败: {}", wxapkg_path, e))?;

    // 前 1024 字节用 AES-256-CBC 解密
    let mut head = [0u8; ENCRYPTED_BLOCK_LEN];
    reader
        .read_exact(&mut head)
        .map_err(|e| format!("读取 {} 加密头失败: {}", wxapkg_path, e))?;
    let mut cipher = Aes256CbcDec::new(GenericArray::from_slice(&key), iv.into());
    for chunk in head.chunks_mut(16) {
        cipher.decrypt_block_mut(GenericArray::from_mut_slice(chunk));
    }

    // 其余字节用单字节 XOR
    let xor_key = wxid
        .as_bytes()
        .iter()
        .nth_back(1)
        .copied()
        .unwrap_or(DEFAULT_XOR_KEY);

    let out = File::create(dec_path).map_err(|e| format!("创建 {} 失败: {}", dec_path, e))?;
    let mut writer = BufWriter::new(out);
    writer
        .write_all(&head[..KEEP_LEN])
        .map_err(|e| format!("写入 {} 失败: {}", dec_path, e))?;

    let mut chunk = [0u8; 16 * 1024];
    loop {
        let n = reader
            .read(&mut chunk)
            .map_err(|e| format!("读取 {} 内容失败: {}", wxapkg_path, e))?;
        if n == 0 {
            break;
        }
        for b in &mut chunk[..n] {
            *b ^= xor_key;
        }
        writer
            .write_all(&chunk[..n])
            .map_err(|e| format!("写入 {} 失败: {}", dec_path, e))?;
    }
    writer
        .flush()
        .map_err(|e| format!("刷新 {} 失败: {}", dec_path, e))?;
    Ok(())
}

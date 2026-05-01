use std::fs;

use aes::Aes256;
use aes::cipher::{BlockDecryptMut, KeyIvInit, generic_array::GenericArray};
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
    let data = fs::read(wxapkg_path).map_err(|e| format!("读取 {} 失败: {}", wxapkg_path, e))?;
    if data.len() < HEADER_OFFSET + ENCRYPTED_BLOCK_LEN {
        return Err("文件过小，不是有效的 wxapkg".to_string());
    }

    let key = pbkdf2_hmac_array::<Sha1, 32>(wxid.as_bytes(), salt, PBKDF2_ROUNDS);

    // 前 1024 字节用 AES-256-CBC 解密
    let (head_enc, tail_enc) = data[HEADER_OFFSET..].split_at(ENCRYPTED_BLOCK_LEN);
    let mut head = head_enc.to_vec();
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

    let mut output = Vec::with_capacity(KEEP_LEN + tail_enc.len());
    output.extend_from_slice(&head[..KEEP_LEN]);
    output.extend(tail_enc.iter().map(|b| b ^ xor_key));

    fs::write(dec_path, output).map_err(|e| format!("写入 {} 失败: {}", dec_path, e))?;
    Ok(())
}

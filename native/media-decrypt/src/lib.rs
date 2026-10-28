use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes128;
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;

const V2_MAGIC: [u8; 6] = [0x07, 0x08, 0x56, 0x32, 0x08, 0x07];
const V2_HEADER_LEN: usize = 15;
const V2_GAP: [u8; 16] = [
    0x11, 0x55, 0xd3, 0x7c, 0xde, 0x38, 0xb6, 0xf1, 0x62, 0x29, 0xf6, 0xc8, 0x3c, 0x36, 0x91,
    0x47,
];

#[napi(object)]
pub struct DecryptDatResult {
    pub data: Buffer,
    pub ext: String,
    #[napi(js_name = "isWxgf")]
    pub is_wxgf: bool,
    pub version: u32,
    #[napi(js_name = "aesSize")]
    pub aes_size: u32,
    #[napi(js_name = "xorSize")]
    pub xor_size: u32,
    #[napi(js_name = "rawSize")]
    pub raw_size: u32,
    pub flag: u32,
}

#[napi(object)]
pub struct DatMeta {
    pub version: Option<u32>,
    #[napi(js_name = "aesSize")]
    pub aes_size: Option<u32>,
    #[napi(js_name = "xorSize")]
    pub xor_size: Option<u32>,
    #[napi(js_name = "rawSize")]
    pub raw_size: Option<u32>,
    pub flag: Option<u32>,
}

fn aes_key_bytes(aes_key: Option<&str>) -> napi::Result<[u8; 16]> {
    let raw = aes_key.unwrap_or("").trim();
    let mut key = [0u8; 16];
    if raw.len() == 16 {
        key.copy_from_slice(raw.as_bytes());
        return Ok(key);
    }
    if raw.len() == 32 && raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        for i in 0..16 {
            key[i] = u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16)
                .map_err(|_| napi::Error::from_reason("invalid aesKey hex"))?;
        }
        return Ok(key);
    }
    Err(napi::Error::from_reason(
        "aesKey must be 16 characters or 32 hex characters",
    ))
}

fn xor_in_place(data: &mut [u8], key: u8) {
    for byte in data.iter_mut() {
        *byte ^= key;
    }
}

fn detect_ext(data: &[u8]) -> &'static str {
    if data.len() >= 3 && data[0] == 0xff && data[1] == 0xd8 && data[2] == 0xff {
        return ".jpg";
    }
    if data.starts_with(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
        return ".png";
    }
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        return ".gif";
    }
    if data.starts_with(b"BM") && data.len() > 14 {
        return ".bmp";
    }
    if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        return ".webp";
    }
    if data.starts_with(&[0x49, 0x49, 0x2a, 0x00]) || data.starts_with(&[0x4d, 0x4d, 0x00, 0x2a]) {
        return ".tif";
    }
    if data.starts_with(b"wxgf") {
        return "wxgf";
    }
    if data.len() >= 12 && &data[4..8] == b"ftyp" {
        let brand = &data[8..12];
        return match brand {
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1" => ".heic",
            b"avif" | b"avis" => ".avif",
            b"qt  " => ".mov",
            _ => ".mp4",
        };
    }
    if data.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return ".webm";
    }
    if data.starts_with(b"#!SILK") || data.starts_with(&[0x02, b'#', b'!', b'S', b'I', b'L', b'K']) {
        return ".silk";
    }
    if data.starts_with(b"#!AMR") {
        return ".amr";
    }
    if data.starts_with(b"ID3") || (data.len() > 1 && data[0] == 0xff && (data[1] & 0xe0) == 0xe0) {
        return ".mp3";
    }
    if data.starts_with(b"%PDF") {
        return ".pdf";
    }
    if data.starts_with(b"PK\x03\x04") {
        return ".zip";
    }
    ""
}

#[napi(js_name = "decryptDatNative")]
pub fn decrypt_dat_native(
    input_path: String,
    xor_key: i64,
    aes_key: Option<String>,
) -> napi::Result<DecryptDatResult> {
    let raw = std::fs::read(&input_path)
        .map_err(|e| napi::Error::from_reason(format!("read {} failed: {}", input_path, e)))?;
    let key = (xor_key & 0xff) as u8;

    if raw.len() >= V2_HEADER_LEN + 16 && raw[..6] == V2_MAGIC {
        let aes_size = u32::from_le_bytes([raw[6], raw[7], raw[8], raw[9]]) as usize;
        let xor_size = u32::from_le_bytes([raw[10], raw[11], raw[12], raw[13]]) as usize;
        let flag = raw[14];
        let expected = V2_HEADER_LEN + aes_size + 16 + xor_size;
        if aes_size % 16 != 0 || expected != raw.len() || aes_size == 0 {
            return Err(napi::Error::from_reason("unsupported V2 layout"));
        }
        let cipher = Aes128::new_from_slice(&aes_key_bytes(aes_key.as_deref())?)
            .map_err(|_| napi::Error::from_reason("invalid aes key"))?;
        let mut plain = raw[V2_HEADER_LEN..V2_HEADER_LEN + aes_size].to_vec();
        for block in plain.chunks_exact_mut(16) {
            cipher.decrypt_block(block.into());
        }
        let mut tail = raw[V2_HEADER_LEN + aes_size + 16..].to_vec();
        xor_in_place(&mut tail, key);
        plain.extend_from_slice(&tail);
        let ext = detect_ext(&plain).to_string();
        let is_wxgf = plain.starts_with(b"wxgf");
        return Ok(DecryptDatResult {
            data: plain.into(),
            ext,
            is_wxgf,
            version: 2,
            aes_size: aes_size as u32,
            xor_size: xor_size as u32,
            raw_size: 0,
            flag: flag as u32,
        });
    }

    if detect_ext(&raw) != "" {
        let ext = detect_ext(&raw).to_string();
        let is_wxgf = raw.starts_with(b"wxgf");
        return Ok(DecryptDatResult {
            data: raw.into(),
            ext,
            is_wxgf,
            version: 0,
            aes_size: 0,
            xor_size: 0,
            raw_size: 0,
            flag: 0,
        });
    }

    let size = raw.len();
    let mut plain = raw;
    xor_in_place(&mut plain, key);
    let ext = detect_ext(&plain).to_string();
    let is_wxgf = plain.starts_with(b"wxgf");
    Ok(DecryptDatResult {
        data: plain.into(),
        ext,
        is_wxgf,
        version: 0,
        aes_size: 0,
        xor_size: size as u32,
        raw_size: 0,
        flag: 0,
    })
}

#[napi(js_name = "encryptDatNative")]
pub fn encrypt_dat_native(
    input_path: String,
    xor_key: i64,
    aes_key: Option<String>,
    meta: Option<DatMeta>,
) -> napi::Result<Buffer> {
    let plain = std::fs::read(&input_path)
        .map_err(|e| napi::Error::from_reason(format!("read {} failed: {}", input_path, e)))?;
    let key = (xor_key & 0xff) as u8;
    let meta = meta.unwrap_or(DatMeta {
        version: None,
        aes_size: None,
        xor_size: None,
        raw_size: None,
        flag: None,
    });
    if meta.version.unwrap_or(0) == 2 && aes_key.is_some() {
        let requested = meta.aes_size.unwrap_or(1024) as usize;
        let aes_size = requested.min(plain.len() / 16 * 16) / 16 * 16;
        if aes_size > 0 {
            let cipher = Aes128::new_from_slice(&aes_key_bytes(aes_key.as_deref())?)
                .map_err(|_| napi::Error::from_reason("invalid aes key"))?;
            let mut out = Vec::with_capacity(plain.len() + 31);
            out.extend_from_slice(&V2_MAGIC);
            out.extend_from_slice(&(aes_size as u32).to_le_bytes());
            out.extend_from_slice(&((plain.len() - aes_size) as u32).to_le_bytes());
            out.push(meta.flag.unwrap_or(1) as u8);
            let mut head = plain[..aes_size].to_vec();
            for block in head.chunks_exact_mut(16) {
                cipher.encrypt_block(block.into());
            }
            out.extend_from_slice(&head);
            out.extend_from_slice(&V2_GAP);
            let mut tail = plain[aes_size..].to_vec();
            xor_in_place(&mut tail, key);
            out.extend_from_slice(&tail);
            return Ok(out.into());
        }
    }
    let mut out = plain;
    xor_in_place(&mut out, key);
    Ok(out.into())
}

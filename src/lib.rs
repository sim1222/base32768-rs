use std::sync::OnceLock;

use thiserror::Error;

const BITS_PER_CHAR: u8 = 15;
const BITS_PER_BYTE: u8 = 8;

use base32768_table as table;

#[derive(Error, Debug)]
pub enum DecodeError {
    #[error("Unrecognised Base32768 character: {0}")]
    UnrecognizedCharacter(char),

    #[error("Secondary character found before end of input at position {0}")]
    UnexpectedSecondaryCharacter(usize),

    #[error("Padding mismatch")]
    PaddingMismatch,
}

pub fn encode(b: &[u8]) -> String {
    if b.is_empty() {
        return String::new();
    }

    let mut code_points: Vec<char> =
        Vec::with_capacity((b.len() * BITS_PER_BYTE as usize).div_ceil(BITS_PER_CHAR as usize));

    let mut buf = 0u16; // left-aligned
    let mut buf_count = 0;
    let mut remaining = 0u16; // right-aligned
    let mut remaining_count = 0;

    for &byte in b {
        // add 8 bits to remaining
        remaining = (remaining << BITS_PER_BYTE) | byte as u16;
        remaining_count += BITS_PER_BYTE;

        if BITS_PER_CHAR - buf_count >= remaining_count {
            buf |= remaining << (BITS_PER_CHAR - buf_count - remaining_count);
            buf_count += remaining_count;
            remaining = 0;
            remaining_count = 0;
        } else {
            let move_count = BITS_PER_CHAR - buf_count;
            let remaining_count_after = remaining_count - move_count;

            buf |= remaining >> remaining_count_after;

            // buf is full (15 bits), push it
            let code_point = table::Z15_REPERTOIRE[buf as usize];
            code_points.push(code_point);
            buf = 0;
            buf_count = 0;

            remaining &= (1 << remaining_count_after) - 1;
            remaining_count = remaining_count_after;
        }
    }

    buf |= remaining << (BITS_PER_CHAR - buf_count - remaining_count);
    buf_count += remaining_count;

    if buf_count >= BITS_PER_BYTE {
        buf |= 0x7F >> (buf_count - BITS_PER_BYTE);
        let code_point = table::Z15_REPERTOIRE[buf as usize];
        code_points.push(code_point);
    } else if buf_count > 0 {
        buf = (buf >> BITS_PER_BYTE) | (0x3F >> (buf_count - 1));
        let code_point = table::Z7_REPERTOIRE[buf as usize];
        code_points.push(code_point);
    }

    code_points.iter().collect()
}

static FAST_LOOKUP_TABLE: OnceLock<Box<[u32; 65536]>> = OnceLock::new();

pub fn decode(src: &str) -> Vec<u8> {
    if src.is_empty() {
        return Vec::new();
    }

    let table_ref = FAST_LOOKUP_TABLE.get_or_init(|| {
        let mut table: Box<[u32; 65536]> = vec![0u32; 65536]
            .into_boxed_slice()
            .try_into()
            .expect("incorrect length");

        // 既存のphf::Map (DECODE_LOOKUP_TABLE) を回して配列化
        for (&c, &(width, val)) in &base32768_table::DECODE_LOOKUP_TABLE {
            let idx = c as usize;
            if idx < 65536 {
                table[idx] = ((width as u32) << 16) | (val as u32);
            }
        }

        table
    });

    let capacity = src.chars().count() * 15 / 8;
    let mut result: Vec<u8> = Vec::with_capacity(capacity);

    unsafe {
        let dst_ptr = result.as_mut_ptr();
        let mut out_offset = 0;
        let mut buf: u64 = 0;
        let mut bit_count: u32 = 0;

        let table_ptr = table_ref.as_ptr();

        for c in src.chars() {
            let c_idx = c as usize;

            // 1. ルックアップ (BMP内のみ)
            if c_idx >= 65536 {
                continue;
            }

            let entry = *table_ptr.add(c_idx);
            let width = entry >> 16; // ビット幅

            // 無効文字(width==0)はスキップ
            if width == 0 {
                continue;
            }

            let val = entry & 0xFFFF; // 値

            // 2. ビット合成
            buf = (buf << width) | (val as u64);
            bit_count += width;

            // 3. バイト抽出 (展開ループ)
            // 最大でも22bit程度しか溜まらないため、ループ回数は知れている
            if bit_count >= 8 {
                bit_count -= 8;
                *dst_ptr.add(out_offset) = (buf >> bit_count) as u8;
                out_offset += 1;

                if bit_count >= 8 {
                    bit_count -= 8;
                    *dst_ptr.add(out_offset) = (buf >> bit_count) as u8;
                    out_offset += 1;
                }
            }
        }

        // 書き込みサイズの確定
        result.set_len(out_offset);
    }

    result
}

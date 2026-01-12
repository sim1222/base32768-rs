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

/// バッファから指定ビット数の値を抽出して出力
#[inline(always)]
fn extract_bytes(buf: &mut u128, bit_count: &mut u32, result: &mut Vec<u8>) {
    // 8バイト (64bit)
    if *bit_count >= 64 {
        *bit_count -= 64;
        let val = ((*buf >> *bit_count) as u64).to_be_bytes();
        result.extend_from_slice(&val);
    }

    // 4バイト (32bit)
    if *bit_count >= 32 {
        *bit_count -= 32;
        let val = ((*buf >> *bit_count) as u32).to_be_bytes();
        result.extend_from_slice(&val);
    }

    // 2バイト (16bit)
    if *bit_count >= 16 {
        *bit_count -= 16;
        let val = ((*buf >> *bit_count) as u16).to_be_bytes();
        result.extend_from_slice(&val);
    }

    // 1バイト (8bit)
    if *bit_count >= 8 {
        *bit_count -= 8;
        result.push((*buf >> *bit_count) as u8);
    }
}

/// ビット値をバッファに追加し、完成したバイトを出力
#[inline(always)]
fn feed_bits(buf: &mut u128, bit_count: &mut u32, width: u32, val: u32, result: &mut Vec<u8>) {
    *buf = (*buf << width) | (val as u128);
    *bit_count += width;
    while *bit_count >= 8 {
        *bit_count -= 8;
        result.push((*buf >> *bit_count) as u8);
    }
}

pub fn decode(src: &str) -> Vec<u8> {
    if src.is_empty() {
        return Vec::new();
    }

    // テーブル取得（初回のみ構築）
    let table_ref = FAST_LOOKUP_TABLE.get_or_init(|| {
        let mut table: Box<[u32; 65536]> = Box::new([0u32; 65536]);

        for (&c, &(width, val)) in &base32768_table::DECODE_LOOKUP_TABLE {
            let idx = c as usize;
            if idx < 65536 {
                table[idx] = ((width as u32) << 16) | (val as u32);
            }
        }

        table
    });

    // 出力サイズの推定: 15bitで1文字, 8bitで1バイト → src.len() * 8 / 15
    let estimated_capacity = (src.len() * 8).saturating_add(14) / 15;
    let mut result = Vec::with_capacity(estimated_capacity);
    let mut buf: u128 = 0;
    let mut bit_count: u32 = 0;

    let chars_vec: Vec<char> = src.chars().collect();
    let len = chars_vec.len();
    let mut i = 0;

    // Fast Path: 8文字チャンクで処理
    while i + 8 <= len && bit_count <= 8 {
        let entries = [
            table_ref[chars_vec[i] as usize],
            table_ref[chars_vec[i + 1] as usize],
            table_ref[chars_vec[i + 2] as usize],
            table_ref[chars_vec[i + 3] as usize],
            table_ref[chars_vec[i + 4] as usize],
            table_ref[chars_vec[i + 5] as usize],
            table_ref[chars_vec[i + 6] as usize],
            table_ref[chars_vec[i + 7] as usize],
        ];

        // すべてが 15bit か判定
        if entries.iter().all(|&e| (e >> 16) == 15) {
            // Fast Path: 15bit x 8 = 120bit を直接パック
            let v0 = (entries[0] & 0xFFFF) as u128;
            let v1 = (entries[1] & 0xFFFF) as u128;
            let v2 = (entries[2] & 0xFFFF) as u128;
            let v3 = (entries[3] & 0xFFFF) as u128;
            let v4 = (entries[4] & 0xFFFF) as u128;
            let v5 = (entries[5] & 0xFFFF) as u128;
            let v6 = (entries[6] & 0xFFFF) as u128;
            let v7 = (entries[7] & 0xFFFF) as u128;

            let packed: u128 = (v0 << 105)
                | (v1 << 90)
                | (v2 << 75)
                | (v3 << 60)
                | (v4 << 45)
                | (v5 << 30)
                | (v6 << 15)
                | v7;

            buf = (buf << 120) | packed;
            bit_count += 120;

            extract_bytes(&mut buf, &mut bit_count, &mut result);
            i += 8;
        } else {
            // Slow Path に切り替え
            break;
        }
    }

    // 残りの文字を処理
    while i < len {
        let idx = chars_vec[i] as usize;
        if idx < 65536 {
            let entry = table_ref[idx];
            let width = entry >> 16;
            if width > 0 {
                let val = entry & 0xFFFF;
                feed_bits(&mut buf, &mut bit_count, width, val, &mut result);
            }
        }
        i += 1;
    }

    result
}

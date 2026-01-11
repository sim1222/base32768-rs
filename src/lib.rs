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

    // テーブル取得（初回のみ構築）
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

    let s_len = src.len();
    let capacity = s_len * 2;
    let mut result: Vec<u8> = Vec::with_capacity(capacity);

    unsafe {
        let dst_ptr = result.as_mut_ptr();
        let mut out_offset = 0;

        let mut buf: u128 = 0;
        let mut bit_count: u32 = 0;

        let table_ptr = table_ref.as_ptr();
        let mut chars = src.chars();

        loop {
            // バッファにゴミが溜まりすぎている場合（稀なケース）、
            // オーバーフローを防ぐために一度手動でSlow Pathへ回して消化させる
            if bit_count > 8 {
                // ここには基本来ないはずですが、安全弁です
                if let Some(c) = chars.next() {
                    let idx = c as usize;
                    if idx < 65536 {
                        let entry = *table_ptr.add(idx);
                        let width = entry >> 16;
                        let val = entry & 0xFFFF;
                        buf = (buf << width) | (val as u128);
                        bit_count += width;
                        while bit_count >= 8 {
                            bit_count -= 8;
                            *dst_ptr.add(out_offset) = (buf >> bit_count) as u8;
                            out_offset += 1;
                        }
                    }
                } else {
                    break;
                }
                continue;
            }

            // 8文字先読み
            let mut chunk_iter = chars.clone();
            let c0 = match chunk_iter.next() {
                Some(c) => c as usize,
                None => break,
            };
            let c1 = match chunk_iter.next() {
                Some(c) => c as usize,
                None => break,
            };
            let c2 = match chunk_iter.next() {
                Some(c) => c as usize,
                None => break,
            };
            let c3 = match chunk_iter.next() {
                Some(c) => c as usize,
                None => break,
            };
            let c4 = match chunk_iter.next() {
                Some(c) => c as usize,
                None => break,
            };
            let c5 = match chunk_iter.next() {
                Some(c) => c as usize,
                None => break,
            };
            let c6 = match chunk_iter.next() {
                Some(c) => c as usize,
                None => break,
            };
            let c7 = match chunk_iter.next() {
                Some(c) => c as usize,
                None => break,
            };
            chars = chunk_iter;

            // 並列ロード
            let e0 = *table_ptr.add(c0);
            let e1 = *table_ptr.add(c1);
            let e2 = *table_ptr.add(c2);
            let e3 = *table_ptr.add(c3);
            let e4 = *table_ptr.add(c4);
            let e5 = *table_ptr.add(c5);
            let e6 = *table_ptr.add(c6);
            let e7 = *table_ptr.add(c7);

            // 全員15bitかチェック
            let combined = (e0 & e1 & e2 & e3 & e4 & e5 & e6 & e7) & 0xFFFF0000;

            if combined == 0x000F0000 {
                // ★ Fast Path: 15bit x 8 = 120bit ★

                let packed: u128 = ((e0 as u128 & 0xFFFF) << 105)
                    | ((e1 as u128 & 0xFFFF) << 90)
                    | ((e2 as u128 & 0xFFFF) << 75)
                    | ((e3 as u128 & 0xFFFF) << 60)
                    | ((e4 as u128 & 0xFFFF) << 45)
                    | ((e5 as u128 & 0xFFFF) << 30)
                    | ((e6 as u128 & 0xFFFF) << 15)
                    | (e7 as u128 & 0xFFFF);

                // ここでオーバーフローしないのは、冒頭の if bit_count > 8 チェックのおかげ
                buf = (buf << 120) | packed;
                bit_count += 120;

                // 【修正点】溜まったビットを可能な限りすべて吐き出す
                // 15バイト(120bit)増えたので、必ず u64, u32, u16, u8 の順で書き出せる

                // 1. 8バイト (64bit) 書き出し
                if bit_count >= 64 {
                    bit_count -= 64;
                    let val = (buf >> bit_count) as u64;
                    (dst_ptr.add(out_offset) as *mut u64).write_unaligned(val.to_be());
                    out_offset += 8;
                }

                // 2. 4バイト (32bit) 書き出し
                if bit_count >= 32 {
                    bit_count -= 32;
                    let val = (buf >> bit_count) as u32;
                    (dst_ptr.add(out_offset) as *mut u32).write_unaligned(val.to_be());
                    out_offset += 4;
                }

                // 3. 2バイト (16bit) 書き出し
                if bit_count >= 16 {
                    bit_count -= 16;
                    let val = (buf >> bit_count) as u16;
                    (dst_ptr.add(out_offset) as *mut u16).write_unaligned(val.to_be());
                    out_offset += 2;
                }

                // 4. 1バイト (8bit) 書き出し
                if bit_count >= 8 {
                    bit_count -= 8;
                    let val = (buf >> bit_count) as u8;
                    *dst_ptr.add(out_offset) = val;
                    out_offset += 1;
                }

                // この時点で bit_count は必ず 8 未満になります。
                // 次のループでのオーバーフローは発生しません。
            } else {
                // Fallback (Slow Path)
                let entries = [e0, e1, e2, e3, e4, e5, e6, e7];
                for &entry in &entries {
                    let width = entry >> 16;
                    let val = entry & 0xFFFF;
                    buf = (buf << width) | (val as u128);
                    bit_count += width;
                    while bit_count >= 8 {
                        bit_count -= 8;
                        *dst_ptr.add(out_offset) = (buf >> bit_count) as u8;
                        out_offset += 1;
                    }
                }
            }
        }

        // 残りの文字処理
        for c in chars {
            let idx = c as usize;
            if idx < 65536 {
                let entry = *table_ptr.add(idx);
                let width = entry >> 16;
                if width > 0 {
                    let val = entry & 0xFFFF;
                    buf = (buf << width) | (val as u128);
                    bit_count += width;
                    while bit_count >= 8 {
                        bit_count -= 8;
                        *dst_ptr.add(out_offset) = (buf >> bit_count) as u8;
                        out_offset += 1;
                    }
                }
            }
        }

        result.set_len(out_offset);
    }
    result
}

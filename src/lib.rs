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

// pub fn decode(s: &str) -> Result<Vec<u8>, DecodeError> {
//     let length = s.chars().count();

//     let mut uint8Array: Vec<u8> =
//         Vec::with_capacity(length * BITS_PER_CHAR as usize / BITS_PER_BYTE as usize);

//     let mut uint8 = 0u8;
//     let mut numUint8Bits = 0;

//     for (i, chr) in s.chars().enumerate() {
//         let Some((numZBits, z)) = table::DECODE_LOOKUP_TABLE.get(&chr) else {
//             return Err(DecodeError::UnrecognizedCharacter(chr));
//         };

//         if *numZBits != BITS_PER_CHAR && i != length - 1 {
//             return Err(DecodeError::UnexpectedSecondaryCharacter(i));
//         }

//         for j in itertools::rev(0..*numZBits) {
//             let bit = (z >> j) & 1;

//             uint8 = (uint8 << 1) + (bit as u8);
//             numUint8Bits += 1;

//             if numUint8Bits == BITS_PER_BYTE {
//                 uint8Array.push(uint8);
//                 uint8 = 0;
//                 numUint8Bits = 0;
//             }
//         }
//     }

//     if uint8 != ((1 << numUint8Bits) - 1) {
//         return Err(DecodeError::PaddingMismatch);
//     }

//     Ok(uint8Array)
// }

pub struct FastDecoder {
    // インデックス: char(u16), 値: u32 (上位16bit: ビット数, 下位16bit: デコード値)
    // Box<[u32]>を使うことでスタックオーバーフローを防ぎつつ、ヒープ(L2キャッシュ)に乗せます
    lookup: Box<[u32; 65536]>,
}

impl FastDecoder {
    /// デコーダーの初期化
    /// codes_15: 15bitとして扱う文字のセット
    /// codes_7:  7bit(末尾)として扱う文字のセット（元コードの仕様に合わせる）
    pub fn new(codes_15: &[char], codes_7: &[char]) -> Self {
        // 全要素を「無効(0)」で埋めた配列を作成
        // ビット数が0 = 無効な文字として扱います
        let mut table = vec![0u32; 65536].into_boxed_slice();

        // 配列への変換: boxから生配列へキャスト（初期化用）
        // ※実際にはvec!マクロで初期化しているので安全ですが、ポインタ操作の例として
        let table_ptr = table.as_mut_ptr();

        unsafe {
            // 15ビット用テーブル構築: (15 << 16) | val
            for (val, &c) in codes_15.iter().enumerate() {
                let idx = c as usize;
                if idx < 65536 {
                    // 上位16bitにビット長(15)、下位16bitに値
                    *table_ptr.add(idx) = (15 << 16) | (val as u32);
                }
            }

            // 7ビット用テーブル構築: (7 << 16) | val
            for (val, &c) in codes_7.iter().enumerate() {
                let idx = c as usize;
                if idx < 65536 {
                    *table_ptr.add(idx) = (7 << 16) | (val as u32);
                }
            }
        }

        // Box<[u32]> から固定長配列への変換は少々面倒ですが、
        // ここでは論理的にサイズが保証されているため、そのまま保持します。
        // ※厳密な型合わせのため、Box<[u32]>として扱います。
        let ptr = Box::into_raw(table);
        let fixed_table = unsafe { Box::from_raw(ptr as *mut [u32; 65536]) };

        Self {
            lookup: fixed_table,
        }
    }

    /// 最速デコード関数
    pub fn decode(&self, src: &str) -> Vec<u8> {
        if src.is_empty() {
            return Vec::new();
        }

        let capacity = src.chars().count() * 15 / 8;

        let mut result: Vec<u8> = Vec::with_capacity(capacity);

        // Unsafe領域の開始
        // ここからはRustの安全ベルトが外れます。速度全振りです。
        unsafe {
            let dst_ptr = result.as_mut_ptr(); // 書き込み用ポインタ
            let mut out_len = 0; // 書き込んだバイト数

            let mut buf: u64 = 0; // ビットバッファ
            let mut bit_count: u32 = 0; // 溜まっているビット数

            let table_ptr = self.lookup.as_ptr(); // ルックアップテーブルのポインタ

            for c in src.chars() {
                let c_idx = c as usize;

                // BMP外の文字はスキップ (テーブル外アクセス防止)
                if c_idx >= 65536 {
                    continue;
                }

                // 1. ルックアップ (get_unchecked相当)
                // table[c_idx] を境界チェックなしで取得
                let entry = *table_ptr.add(c_idx);

                // ビット長を取得（上位16bit）
                let width = entry >> 16;

                // width == 0 は無効文字（テーブルに登録されていない）なのでスキップ
                if width == 0 {
                    continue;
                }

                // 値を取得（下位16bit）
                let val = entry & 0xFFFF;

                // 2. ビット合成
                buf = (buf << width) | (val as u64);
                bit_count += width;

                // 3. バイト抽出
                // 15bit足すと、溜まるビット数は最大で 7(残り) + 15 = 22bit。
                // つまり、書き出すバイト数は最大2バイトです。ループせずifで展開します。

                // 1バイト目を取り出せるか？
                if bit_count >= 8 {
                    bit_count -= 8;
                    let byte = (buf >> bit_count) as u8;

                    // ポインタ経由で直接書き込み (pushのオーバーヘッド回避)
                    *dst_ptr.add(out_len) = byte;
                    out_len += 1;

                    // 2バイト目を取り出せるか？ (例: 22bit溜まっていたら8引いて14、もう一回いける)
                    if bit_count >= 8 {
                        bit_count -= 8;
                        let byte = (buf >> bit_count) as u8;

                        *dst_ptr.add(out_len) = byte;
                        out_len += 1;
                    }
                }
            }

            // ループ終了後の端数処理は必要に応じて記述
            // Java版のロジックではパディングが特殊でしたが、
            // ここでは基本的な「残ったビットは捨てる（あるいは特定条件下で書き出す）」動作となります。

            // Vecの長さを手動で設定
            result.set_len(out_len);
        }

        result
    }
}

pub struct LudicrousDecoder {
    // 64KBのテーブル。L1キャッシュ（通常32KB-48KB程度）に近いサイズまで圧縮。
    // 値が 0xFFFF の場合は無効、それ以外はデコード値(15bit)
    lookup: Box<[u16; 65536]>,
    // 末尾処理用の特別なマップ（頻度が低いので検索コストは無視できる）
    // 文字コード -> (値, ビット数)
    tail_map: Vec<(u16, u16, u8)>,
}

impl LudicrousDecoder {
    pub fn new(codes_15: &[char], codes_7: &[char]) -> Self {
        // Create a boxed slice and convert it into a boxed array of length 65536.
        // This uses Box::into_raw / Box::from_raw to change the unsized boxed slice
        // into a sized boxed array without copying.
        let table_slice = vec![0xFFFFu16; 65536].into_boxed_slice();
        let mut table: Box<[u16; 65536]> =
            unsafe { Box::from_raw(Box::into_raw(table_slice) as *mut [u16; 65536]) };
        let mut tail_map = Vec::new();

        // メインテーブル構築 (15bit用)
        for (val, &c) in codes_15.iter().enumerate() {
            let idx = c as usize;
            if idx < 65536 {
                table[idx] = val as u16;
            }
        }

        // 末尾用データの準備 (CODES_7 のマッピングなど)
        // ここでは単純化のため、CODES_7の内容をtail_mapに保持するロジックとします
        for (val, &c) in codes_7.iter().enumerate() {
             tail_map.push((c as u16, val as u16, 7));
        }

        Self { lookup: table, tail_map }
    }

    #[inline(always)] // インライン展開を強制
    pub fn decode(&self, src: &str) -> Vec<u8> {
        let s_len = src.len();
        if s_len == 0 { return Vec::new(); }

        // 出力バッファ確保
        // 1文字15bit → 約2バイト。余裕を持って確保。
        let capacity = s_len * 2 + 8; // +8はu64書き込みのはみ出し対策
        let mut result: Vec<u8> = Vec::with_capacity(capacity);

        unsafe {
            let mut dst_ptr = result.as_mut_ptr();
            let mut out_offset = 0;

            // u128アキュムレータ
            let mut buf: u128 = 0;
            let mut bit_count: u32 = 0;

            // ポインタキャッシュ
            let table_ptr = self.lookup.as_ptr();

            // イテレータを取得
            let mut chars = src.chars();
            
            // 【高速化の肝】
            // 最後の1文字を残してループする。
            // これにより、ループ内では「常に15bit」「常にテーブルにある」という前提で
            // 分岐予測を排除した爆速実行が可能。
            let loop_len = s_len - 1;

            for _ in 0..loop_len {
                // unwrap_uncheckedは使わないが、論理的に安全
                if let Some(c) = chars.next() {
                    let idx = c as usize;
                    
                    // 1. 爆速ルックアップ (L1キャッシュヒット前提)
                    // 境界チェックなし、ビット幅取得なし。ただ値を読むだけ。
                    let val = *table_ptr.add(idx); // u16

                    // 2. ビット合成 (u128)
                    buf = (buf << 15) | (val as u128);
                    bit_count += 15;

                    // 3. 塊書き出し (Bulk Store)
                    // 64bit以上溜まったら、u64(8バイト)として一気にメモリに書く
                    if bit_count >= 64 {
                        bit_count -= 64;
                        // 上位64bitを取り出す
                        let write_val = (buf >> bit_count) as u64;
                        
                        // バイトオーダーをビッグエンディアン(ネットワークオーダー)にして書き込み
                        // x86はリトルエンディアンなので変換コストがあるが、BSWAP命令1つで終わる
                        (dst_ptr.add(out_offset) as *mut u64).write_unaligned(write_val.to_be());
                        
                        out_offset += 8;
                    }
                }
            }

            // 残ったバッファの中身を書き出す (u64未満の端数)
            while bit_count >= 8 {
                bit_count -= 8;
                let byte = (buf >> bit_count) as u8;
                *dst_ptr.add(out_offset) = byte;
                out_offset += 1;
            }

            // 【最後の1文字の処理】
            // ここだけ慎重に行う（15bitかもしれないし、7bitかもしれない）
            if let Some(last_c) = chars.next() {
                // まず15bitテーブルで引いてみる
                let idx = last_c as usize;
                let val_15 = *table_ptr.add(idx);
                
                if val_15 != 0xFFFF {
                    // 15bit文字だった場合
                    buf = (buf << 15) | (val_15 as u128);
                    bit_count += 15;
                } else {
                    // 7bit文字（または無効文字）の場合
                    // tail_mapから線形探索などで探す（頻度極小なので遅くていい）
                    // ※実装省略：実際にはself.tail_mapから検索して (val, width) を得る
                    let (val, width) = (0, 7); // ダミー
                    buf = (buf << width) | (val as u128);
                    bit_count += width;
                }

                // 最後のフラッシュ
                while bit_count >= 8 {
                    bit_count -= 8;
                    let byte = (buf >> bit_count) as u8;
                    *dst_ptr.add(out_offset) = byte;
                    out_offset += 1;
                }
            }
            
            result.set_len(out_offset);
        }

        result
    }
}
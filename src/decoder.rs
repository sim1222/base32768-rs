use crate::DecodeError;

// =============================================================================
// 定数
// =============================================================================

/// 7ビット文字（終端文字用）の最大コードポイント
const MAX_7BIT_CODEPOINT: u32 = 0x29F;

/// 通常文字のビット幅
const BITS_PER_CHAR: u32 = 15;

/// 7ビットフラグ（デコードテーブルの値に含まれる）
const SEVEN_BIT_FLAG: u16 = 0x8000;

/// 高速パスで一度に処理する文字数
const FAST_PATH_CHARS: usize = 8;

/// 高速パスで一度に出力するバイト数 (8文字 × 15ビット = 120ビット = 15バイト)
const FAST_PATH_BYTES: usize = 15;

// =============================================================================
// 公開API
// =============================================================================

/// Base32768エンコードされた文字列をデコードしてバイト列に変換する
pub fn decode(src: &str) -> Result<Vec<u8>, DecodeError> {
    if src.is_empty() {
        return Ok(Vec::new());
    }

    let chars: Vec<u16> = src.encode_utf16().collect();
    if chars.is_empty() {
        return Ok(Vec::new());
    }

    let decode_table = &base32768_table::FAST_DECODE_LOOKUP_TABLE;

    // 出力バッファのサイズを計算
    let output_len = calculate_output_length(&chars, decode_table)?;
    let mut output = vec![0u8; output_len];

    // デコード処理
    let mut state = DecodeState::new(&chars);
    decode_main_loop(&mut state, &chars, decode_table, &mut output)?;
    decode_last_char(&mut state, &chars, decode_table, &mut output)?;
    validate_padding(&state)?;

    Ok(output)
}

// =============================================================================
// 内部状態管理
// =============================================================================

/// デコード処理の状態を管理する構造体
struct DecodeState {
    /// 入力文字列のインデックス
    src_index: usize,
    /// 出力バッファのインデックス
    out_index: usize,
    /// ビットアキュムレータ（未出力のビットを保持）
    accumulator: u64,
    /// アキュムレータ内の有効ビット数
    bit_count: u32,
    /// 最後の文字を除いた文字数
    chars_before_last: usize,
}

impl DecodeState {
    fn new(chars: &[u16]) -> Self {
        Self {
            src_index: 0,
            out_index: 0,
            accumulator: 0,
            bit_count: 0,
            chars_before_last: chars.len().saturating_sub(1),
        }
    }
}

// =============================================================================
// 出力サイズ計算
// =============================================================================

/// 出力バッファのサイズを計算する
fn calculate_output_length(chars: &[u16], decode_table: &[u32]) -> Result<usize, DecodeError> {
    let n = chars.len();
    let last_char = chars[n - 1] as u32;

    let last_bits = get_char_bit_width(last_char, decode_table)?;

    // 最後の文字が7ビット文字かどうかで計算方法が異なる
    let is_7bit_final = last_char <= MAX_7BIT_CODEPOINT;
    let total_bits = if is_7bit_final {
        // 7ビット終端: (n-1)文字 × 15ビット + 最後のビット幅
        (n as u32 - 1) * BITS_PER_CHAR + last_bits
    } else {
        // 15ビット終端: n文字 × 15ビット
        n as u32 * BITS_PER_CHAR
    };

    Ok((total_bits / 8) as usize)
}

/// 文字のビット幅を取得する
fn get_char_bit_width(codepoint: u32, decode_table: &[u32]) -> Result<u32, DecodeError> {
    if codepoint >= 0x10000 {
        return Err(DecodeError::UnrecognizedCharacter(
            char::from_u32(codepoint).unwrap_or('\0'),
        ));
    }

    let entry = decode_table[codepoint as usize];
    let bits = (entry >> 16) & 0xFFFF;

    if bits == 0 {
        return Err(DecodeError::UnrecognizedCharacter(
            char::from_u32(codepoint).unwrap_or('\0'),
        ));
    }

    Ok(bits)
}

// =============================================================================
// メインデコードループ
// =============================================================================

/// メインのデコードループ（最後の文字を除く）
fn decode_main_loop(
    state: &mut DecodeState,
    chars: &[u16],
    decode_table: &[u32],
    output: &mut [u8],
) -> Result<(), DecodeError> {
    // 高速パス: 8文字単位で処理
    decode_fast_path_8chars(state, chars, decode_table, output)?;

    // 中速パス: 2文字単位で処理
    decode_medium_path_2chars(state, chars, decode_table, output)?;

    // 残り1文字の処理
    decode_remaining_single_char(state, chars, decode_table, output)?;

    Ok(())
}

// =============================================================================
// 高速パス (8文字単位)
// =============================================================================

/// 8文字を一度に処理する高速パス
fn decode_fast_path_8chars(
    state: &mut DecodeState,
    chars: &[u16],
    decode_table: &[u32],
    output: &mut [u8],
) -> Result<(), DecodeError> {
    // 8文字単位で処理できる範囲を計算
    let fast_end = state.chars_before_last & !7;

    while state.src_index < fast_end {
        let values = decode_8_chars(&chars[state.src_index..], decode_table)?;
        validate_no_7bit_flag(&values, chars[state.src_index])?;
        write_15_bytes_from_8_values(&values, output, state.out_index);

        state.src_index += FAST_PATH_CHARS;
        state.out_index += FAST_PATH_BYTES;
    }

    Ok(())
}

/// 8文字をデコードして値の配列を返す
fn decode_8_chars(chars: &[u16], decode_table: &[u32]) -> Result<[u16; 8], DecodeError> {
    Ok([
        decode_char(chars[0], decode_table)?,
        decode_char(chars[1], decode_table)?,
        decode_char(chars[2], decode_table)?,
        decode_char(chars[3], decode_table)?,
        decode_char(chars[4], decode_table)?,
        decode_char(chars[5], decode_table)?,
        decode_char(chars[6], decode_table)?,
        decode_char(chars[7], decode_table)?,
    ])
}

/// 7ビットフラグが含まれていないことを確認
fn validate_no_7bit_flag(values: &[u16; 8], first_char: u16) -> Result<(), DecodeError> {
    let combined = values[0] | values[1] | values[2] | values[3]
                 | values[4] | values[5] | values[6] | values[7];

    if (combined & SEVEN_BIT_FLAG) != 0 {
        return Err(DecodeError::UnrecognizedCharacter(
            char::from_u32(first_char as u32).unwrap_or('\0'),
        ));
    }
    Ok(())
}

/// 8つの15ビット値から15バイトを出力バッファに書き込む
fn write_15_bytes_from_8_values(values: &[u16; 8], output: &mut [u8], offset: usize) {
    let [v0, v1, v2, v3, v4, v5, v6, v7] = *values;

    // 120ビット(15バイト)を2つの64ビット値に分割して書き込む
    // w0: v0(15) + v1(15) + v2(15) + v3(15) + v4の上位4ビット = 64ビット
    let w0: u64 = ((v0 as u64) << 49)
        | ((v1 as u64) << 34)
        | ((v2 as u64) << 19)
        | ((v3 as u64) << 4)
        | ((v4 as u64) >> 11);

    // w1: 7バイト目の重複 + v4の下位11ビット + v5(15) + v6(15) + v7(15)
    let w1: u64 = ((w0 & 0xFF) << 56)
        | (((v4 as u64) & 0x7FF) << 45)
        | ((v5 as u64) << 30)
        | ((v6 as u64) << 15)
        | (v7 as u64);

    write_u64_be(output, offset, w0);
    write_u64_be(output, offset + 7, w1);
}

// =============================================================================
// 中速パス (2文字単位)
// =============================================================================

/// 2文字を一度に処理する中速パス
fn decode_medium_path_2chars(
    state: &mut DecodeState,
    chars: &[u16],
    decode_table: &[u32],
    output: &mut [u8],
) -> Result<(), DecodeError> {
    let limit = state.chars_before_last.saturating_sub(1);

    while state.src_index < limit {
        let v0 = decode_char(chars[state.src_index], decode_table)?;
        let v1 = decode_char(chars[state.src_index + 1], decode_table)?;

        if (v0 | v1) & SEVEN_BIT_FLAG != 0 {
            return Err(DecodeError::UnrecognizedCharacter(
                char::from_u32(chars[state.src_index] as u32).unwrap_or('\0'),
            ));
        }

        // 2文字分(30ビット)をアキュムレータに追加
        state.accumulator = (state.accumulator << 30) | (((v0 as u64) << 15) | (v1 as u64));
        state.bit_count += 30;

        // 可能な限りバイトを出力
        flush_bytes(state, output);

        state.src_index += 2;
    }

    Ok(())
}

/// アキュムレータから可能な限りバイトを出力
fn flush_bytes(state: &mut DecodeState, output: &mut [u8]) {
    // 3バイト一括出力
    if state.out_index + 3 <= output.len() && state.bit_count >= 24 {
        output[state.out_index] = (state.accumulator >> (state.bit_count - 8)) as u8;
        output[state.out_index + 1] = (state.accumulator >> (state.bit_count - 16)) as u8;
        output[state.out_index + 2] = (state.accumulator >> (state.bit_count - 24)) as u8;
        state.out_index += 3;
        state.bit_count -= 24;
    }

    // 残りの1バイト出力
    if state.bit_count >= 8 && state.out_index < output.len() {
        output[state.out_index] = (state.accumulator >> (state.bit_count - 8)) as u8;
        state.out_index += 1;
        state.bit_count -= 8;
    }
}

// =============================================================================
// 残り文字の処理
// =============================================================================

/// 残り1文字を処理
fn decode_remaining_single_char(
    state: &mut DecodeState,
    chars: &[u16],
    decode_table: &[u32],
    output: &mut [u8],
) -> Result<(), DecodeError> {
    if state.src_index >= state.chars_before_last {
        return Ok(());
    }

    let value = decode_char(chars[state.src_index], decode_table)?;
    if (value & SEVEN_BIT_FLAG) != 0 {
        return Err(DecodeError::UnrecognizedCharacter(
            char::from_u32(chars[state.src_index] as u32).unwrap_or('\0'),
        ));
    }

    state.accumulator = (state.accumulator << BITS_PER_CHAR) | (value as u64);
    state.bit_count += BITS_PER_CHAR;

    // 最大2バイト出力
    for _ in 0..2 {
        if state.bit_count >= 8 && state.out_index < output.len() {
            output[state.out_index] = (state.accumulator >> (state.bit_count - 8)) as u8;
            state.out_index += 1;
            state.bit_count -= 8;
        }
    }

    Ok(())
}

// =============================================================================
// 最後の文字の処理
// =============================================================================

/// 最後の文字をデコードして残りのバイトを出力
fn decode_last_char(
    state: &mut DecodeState,
    chars: &[u16],
    decode_table: &[u32],
    output: &mut [u8],
) -> Result<(), DecodeError> {
    let last_index = chars.len() - 1;
    let last_char = chars[last_index] as u32;

    let last_value = decode_char(chars[last_index], decode_table)?;
    let last_bits = get_char_bit_width(last_char, decode_table)?;

    // 7ビットフラグを除去した値を使用
    let value_without_flag = last_value & 0x7FFF;

    state.accumulator = (state.accumulator << last_bits) | (value_without_flag as u64);
    state.bit_count += last_bits;

    // 残りのバイトを全て出力
    while state.bit_count >= 8 && state.out_index < output.len() {
        state.bit_count -= 8;
        output[state.out_index] = (state.accumulator >> state.bit_count) as u8;
        state.out_index += 1;
    }

    Ok(())
}

// =============================================================================
// パディング検証
// =============================================================================

/// パディングビットが正しいことを検証
fn validate_padding(state: &DecodeState) -> Result<(), DecodeError> {
    if state.bit_count == 0 {
        return Ok(());
    }

    // パディングビットは全て1でなければならない
    let padding_mask = (1u64 << state.bit_count) - 1;
    let padding_bits = state.accumulator & padding_mask;

    if padding_bits != padding_mask {
        return Err(DecodeError::PaddingMismatch);
    }

    Ok(())
}

// =============================================================================
// ユーティリティ関数
// =============================================================================

/// 64ビット値をビッグエンディアンで出力バッファに書き込む
#[inline(always)]
fn write_u64_be(output: &mut [u8], offset: usize, value: u64) {
    if offset + 8 <= output.len() {
        output[offset] = (value >> 56) as u8;
        output[offset + 1] = (value >> 48) as u8;
        output[offset + 2] = (value >> 40) as u8;
        output[offset + 3] = (value >> 32) as u8;
        output[offset + 4] = (value >> 24) as u8;
        output[offset + 5] = (value >> 16) as u8;
        output[offset + 6] = (value >> 8) as u8;
        output[offset + 7] = value as u8;
    }
}

/// 1文字をデコードして値を返す
#[inline(always)]
fn decode_char(char_code: u16, decode_table: &[u32]) -> Result<u16, DecodeError> {
    let entry = decode_table[char_code as usize];
    let bits = (entry >> 16) & 0xFFFF;

    if bits == 0 {
        return Err(DecodeError::UnrecognizedCharacter(
            char::from_u32(char_code as u32).unwrap_or('\0'),
        ));
    }

    Ok((entry & 0xFFFF) as u16)
}

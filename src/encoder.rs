use base32768_table as table;

const BITS_PER_CHAR: u8 = 15;
const BITS_PER_BYTE: u8 = 8;

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

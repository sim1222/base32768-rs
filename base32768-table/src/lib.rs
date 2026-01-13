#[rustfmt::skip]
#[allow(clippy::all)]
#[allow(warnings)]
mod table {
	include!(concat!(env!("OUT_DIR"), "/table.rs"));
}

pub use table::{DECODE_LOOKUP_TABLE, Z15_REPERTOIRE, Z7_REPERTOIRE, FAST_DECODE_LOOKUP_TABLE};

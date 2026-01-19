use thiserror::Error;

#[derive(Error, Debug)]
pub enum DecodeError {
    #[error("Unrecognized Base32768 character: {0}")]
    UnrecognizedCharacter(char),

    #[error("Secondary character found before end of input at position {0}")]
    UnexpectedSecondaryCharacter(usize),

    #[error("Padding mismatch")]
    PaddingMismatch,
}

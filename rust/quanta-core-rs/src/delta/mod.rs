//! Delta wire format: bit-level I/O, headers, field bitmasks, and encoding.

mod bit_reader;
mod bit_writer;
mod bitmask;
pub mod encoder;
mod header;

pub use bit_reader::BitReader;
pub use bit_writer::BitWriter;
pub use bitmask::FieldBitmask;
pub use header::{DeltaHeader, HEADER_SIZE};

use std::fmt;

/// Errors produced by delta encoding/decoding operations.
#[derive(Debug, PartialEq)]
pub enum DeltaError {
    ReadPastEnd { requested: u8, remaining: u32 },
    TruncatedHeader { expected: usize, got: usize },
    InvalidBitmaskLength { expected: usize, got: usize },
    SchemaVersionMismatch { expected: u8, got: u8 },
    StateTooShort { expected: usize, got: usize },
    TruncatedDelta,
    NaNOrInfinity { field: String },
    FieldCountMismatch { expected: usize, got: usize },
    UnsupportedDeltaFormat,
    PayloadBitsMismatch { expected: u32, got: u16 },
}

impl fmt::Display for DeltaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadPastEnd {
                requested,
                remaining,
            } => write!(
                f,
                "read past end: requested {requested} bits, {remaining} remaining"
            ),
            Self::TruncatedHeader { expected, got } => {
                write!(f, "truncated header: expected {expected} bytes, got {got}")
            }
            Self::InvalidBitmaskLength { expected, got } => {
                write!(
                    f,
                    "invalid bitmask length: expected {expected} bytes, got {got}"
                )
            }
            Self::SchemaVersionMismatch { expected, got } => {
                write!(f, "schema version mismatch: expected {expected}, got {got}")
            }
            Self::StateTooShort { expected, got } => {
                write!(f, "state too short: expected {expected} bytes, got {got}")
            }
            Self::TruncatedDelta => write!(f, "truncated delta"),
            Self::NaNOrInfinity { field } => {
                write!(f, "NaN or infinity value for field: {field}")
            }
            Self::FieldCountMismatch { expected, got } => {
                write!(f, "field count mismatch: expected {expected}, got {got}")
            }
            Self::UnsupportedDeltaFormat => {
                write!(f, "unsupported delta format (full snapshot or compressed)")
            }
            Self::PayloadBitsMismatch { expected, got } => {
                write!(f, "payload bits mismatch: expected {expected}, got {got}")
            }
        }
    }
}

impl std::error::Error for DeltaError {}

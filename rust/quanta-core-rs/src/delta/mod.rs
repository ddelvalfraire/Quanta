//! Delta wire format: bit-level I/O, headers, and field bitmasks.

mod bit_reader;
mod bit_writer;
mod bitmask;
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
        }
    }
}

impl std::error::Error for DeltaError {}

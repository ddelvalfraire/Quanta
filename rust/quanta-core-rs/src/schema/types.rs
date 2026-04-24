use std::fmt;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CompileOptions {
    pub prediction_enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledSchema {
    pub version: u8,
    pub fields: Vec<FieldMeta>,
    pub field_groups: Vec<FieldGroup>,
    pub total_bits: u32,
    pub bitmask_byte_count: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldMeta {
    pub name: String,
    pub field_type: FieldType,
    pub bit_width: u8,
    pub bit_offset: u32,
    pub group_index: u8,
    pub quantization: Option<QuantizationParams>,
    pub prediction: PredictionMode,
    pub smoothing: Option<SmoothingParams>,
    pub interpolation: InterpolationMode,
    pub skip_delta: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldGroup {
    pub name: String,
    pub priority: Priority,
    pub max_tick_rate: u16,
    pub bitmask_range: (u16, u16),
}

#[derive(Debug, Clone, PartialEq)]
pub struct QuantizationParams {
    pub min: f64,
    pub max: f64,
    pub precision: f64,
    pub num_values: u64,
    pub mask: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SmoothingParams {
    pub mode: SmoothingMode,
    pub duration_ms: u32,
    pub max_distance: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Bool,
    U8,
    S8,
    U16,
    S16,
    U32,
    S32,
    U64,
    S64,
    F32,
    F64,
    String,
    Enum(u16),
    Flags(u16),
}

impl FieldType {
    pub fn from_byte(byte: u8, variant_count: u16) -> Option<Self> {
        match byte {
            0 => Some(FieldType::Bool),
            1 => Some(FieldType::U8),
            2 => Some(FieldType::S8),
            3 => Some(FieldType::U16),
            4 => Some(FieldType::S16),
            5 => Some(FieldType::U32),
            6 => Some(FieldType::S32),
            7 => Some(FieldType::U64),
            8 => Some(FieldType::S64),
            9 => Some(FieldType::F32),
            10 => Some(FieldType::F64),
            11 => Some(FieldType::String),
            12 => Some(FieldType::Enum(variant_count)),
            13 => Some(FieldType::Flags(variant_count)),
            _ => None,
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            FieldType::U8
                | FieldType::S8
                | FieldType::U16
                | FieldType::S16
                | FieldType::U32
                | FieldType::S32
                | FieldType::U64
                | FieldType::S64
                | FieldType::F32
                | FieldType::F64
        )
    }

    pub fn is_float(&self) -> bool {
        matches!(self, FieldType::F32 | FieldType::F64)
    }

    pub fn native_bits(&self) -> Option<u8> {
        match self {
            FieldType::Bool => Some(1),
            FieldType::U8 | FieldType::S8 => Some(8),
            FieldType::U16 | FieldType::S16 => Some(16),
            FieldType::U32 | FieldType::S32 => Some(32),
            FieldType::U64 | FieldType::S64 => Some(64),
            FieldType::F32 => Some(32),
            FieldType::F64 => Some(64),
            FieldType::String => None,
            FieldType::Enum(n) => {
                if *n <= 1 {
                    Some(0)
                } else {
                    Some(ceil_log2_u16(*n))
                }
            }
            FieldType::Flags(n) => Some(*n as u8),
        }
    }

    pub fn type_byte(&self) -> u8 {
        match self {
            FieldType::Bool => 0,
            FieldType::U8 => 1,
            FieldType::S8 => 2,
            FieldType::U16 => 3,
            FieldType::S16 => 4,
            FieldType::U32 => 5,
            FieldType::S32 => 6,
            FieldType::U64 => 7,
            FieldType::S64 => 8,
            FieldType::F32 => 9,
            FieldType::F64 => 10,
            FieldType::String => 11,
            FieldType::Enum(_) => 12,
            FieldType::Flags(_) => 13,
        }
    }
}

fn ceil_log2_u16(n: u16) -> u8 {
    if n <= 1 {
        return 0;
    }
    (16 - (n - 1).leading_zeros()) as u8
}

pub fn ceil_log2_u64(n: u64) -> u8 {
    if n <= 1 {
        return 0;
    }
    (64 - (n - 1).leading_zeros()) as u8
}

/// Compute quantized bit count from precision and clamp range.
/// Returns `None` if the range overflows `u64`.
pub fn quantize_bits(precision: f64, min: f64, max: f64) -> Option<(u64, u8)> {
    let raw = ((max - min) / precision).floor();
    if !raw.is_finite() || raw < 0.0 || raw >= u64::MAX as f64 {
        return None;
    }
    let num_values = (raw as u64).checked_add(1)?;
    let bits = ceil_log2_u64(num_values);
    Some((num_values, bits))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Critical = 0,
    High = 1,
    Medium = 2,
    Low = 3,
}

impl Priority {
    pub fn as_byte(&self) -> u8 {
        *self as u8
    }

    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Priority::Critical),
            1 => Some(Priority::High),
            2 => Some(Priority::Medium),
            3 => Some(Priority::Low),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredictionMode {
    None,
    InputReplay,
    Cosmetic,
}

impl PredictionMode {
    pub fn as_byte(&self) -> u8 {
        match self {
            PredictionMode::None => 0,
            PredictionMode::InputReplay => 1,
            PredictionMode::Cosmetic => 2,
        }
    }

    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(PredictionMode::None),
            1 => Some(PredictionMode::InputReplay),
            2 => Some(PredictionMode::Cosmetic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmoothingMode {
    Lerp,
    Snap,
    SnapLerp,
}

impl SmoothingMode {
    pub fn as_byte(&self) -> u8 {
        match self {
            SmoothingMode::Lerp => 0,
            SmoothingMode::Snap => 1,
            SmoothingMode::SnapLerp => 2,
        }
    }

    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(SmoothingMode::Lerp),
            1 => Some(SmoothingMode::Snap),
            2 => Some(SmoothingMode::SnapLerp),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpolationMode {
    None,
    Linear,
    Hermite,
}

impl InterpolationMode {
    pub fn as_byte(&self) -> u8 {
        match self {
            InterpolationMode::None => 0,
            InterpolationMode::Linear => 1,
            InterpolationMode::Hermite => 2,
        }
    }

    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(InterpolationMode::None),
            1 => Some(InterpolationMode::Linear),
            2 => Some(InterpolationMode::Hermite),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SchemaError {
    TypeNotFound(String),
    QuantizeOnNonNumeric { field: String },
    InterpolateOnNonNumeric { field: String },
    PrecisionNotPositive { field: String },
    ClampMinGeMax { field: String, min: f64, max: f64 },
    QuantizeClampBitsTooLarge { field: String, bits: u32 },
    StringWithoutSkipDelta { field: String },
    SmoothLerpOnNonNumeric { field: String },
    PredictionNotEnabled { field: String },
    ParseError(String),
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemaError::TypeNotFound(name) => write!(f, "type not found: {name}"),
            SchemaError::QuantizeOnNonNumeric { field } => {
                write!(f, "quantize on non-numeric field: {field}")
            }
            SchemaError::InterpolateOnNonNumeric { field } => {
                write!(f, "interpolate on non-numeric field: {field}")
            }
            SchemaError::PrecisionNotPositive { field } => {
                write!(f, "precision must be > 0 for field: {field}")
            }
            SchemaError::ClampMinGeMax { field, min, max } => {
                write!(f, "clamp min ({min}) >= max ({max}) for field: {field}")
            }
            SchemaError::QuantizeClampBitsTooLarge { field, bits } => {
                write!(
                    f,
                    "quantize+clamp requires {bits} bits (> 64) for field: {field}"
                )
            }
            SchemaError::StringWithoutSkipDelta { field } => {
                write!(f, "string field '{field}' must have @quanta:skip_delta")
            }
            SchemaError::SmoothLerpOnNonNumeric { field } => {
                write!(f, "smooth(lerp|snap_lerp) on non-numeric field: {field}")
            }
            SchemaError::PredictionNotEnabled { field } => {
                write!(
                    f,
                    "predict(input_replay|cosmetic) without prediction enabled for field: {field}"
                )
            }
            SchemaError::ParseError(msg) => write!(f, "parse error: {msg}"),
        }
    }
}

impl std::error::Error for SchemaError {}

#[derive(Debug, Clone, PartialEq)]
pub enum SchemaWarning {
    QuantizeWithoutClamp {
        field: String,
    },
    BitsGeNativeWidth {
        field: String,
        computed: u8,
        native: u8,
    },
    RedundantClamp {
        field: String,
    },
    UnknownAnnotation {
        field: String,
        annotation: String,
    },
    MalformedAnnotation {
        field: String,
        directive: String,
    },
    PredictOnNonNumeric {
        field: String,
    },
}

impl fmt::Display for SchemaWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemaWarning::QuantizeWithoutClamp { field } => {
                write!(f, "quantize without clamp on field: {field}")
            }
            SchemaWarning::BitsGeNativeWidth {
                field,
                computed,
                native,
            } => {
                write!(
                    f,
                    "computed width ({computed}) >= native width ({native}) for field: {field}"
                )
            }
            SchemaWarning::RedundantClamp { field } => {
                write!(f, "redundant clamp on field: {field}")
            }
            SchemaWarning::UnknownAnnotation { field, annotation } => {
                write!(
                    f,
                    "unknown annotation @quanta:{annotation} on field: {field}"
                )
            }
            SchemaWarning::MalformedAnnotation { field, directive } => {
                write!(
                    f,
                    "malformed @quanta:{directive} annotation on field: {field}"
                )
            }
            SchemaWarning::PredictOnNonNumeric { field } => {
                write!(f, "predict on non-numeric field: {field}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ceil_log2_u64_edge_cases() {
        assert_eq!(ceil_log2_u64(0), 0);
        assert_eq!(ceil_log2_u64(1), 0);
        assert_eq!(ceil_log2_u64(2), 1);
        assert_eq!(ceil_log2_u64(3), 2);
        assert_eq!(ceil_log2_u64(4), 2);
        assert_eq!(ceil_log2_u64(5), 3);
        assert_eq!(ceil_log2_u64(256), 8);
        assert_eq!(ceil_log2_u64(257), 9);
        assert_eq!(ceil_log2_u64(2_000_001), 21);
    }

    #[test]
    fn quantize_bits_normal() {
        let (nv, bits) = quantize_bits(0.01, -10000.0, 10000.0).unwrap();
        assert_eq!(nv, 2_000_001);
        assert_eq!(bits, 21);
    }

    #[test]
    fn quantize_bits_overflow_returns_none() {
        assert!(quantize_bits(0.0000001, -1e18, 1e18).is_none());
    }

    #[test]
    fn field_type_native_bits() {
        assert_eq!(FieldType::Bool.native_bits(), Some(1));
        assert_eq!(FieldType::U8.native_bits(), Some(8));
        assert_eq!(FieldType::S8.native_bits(), Some(8));
        assert_eq!(FieldType::U16.native_bits(), Some(16));
        assert_eq!(FieldType::U32.native_bits(), Some(32));
        assert_eq!(FieldType::U64.native_bits(), Some(64));
        assert_eq!(FieldType::F32.native_bits(), Some(32));
        assert_eq!(FieldType::F64.native_bits(), Some(64));
        assert_eq!(FieldType::String.native_bits(), None);
        assert_eq!(FieldType::Enum(1).native_bits(), Some(0));
        assert_eq!(FieldType::Enum(4).native_bits(), Some(2));
        assert_eq!(FieldType::Flags(6).native_bits(), Some(6));
    }

    #[test]
    fn field_type_is_numeric() {
        assert!(FieldType::F32.is_numeric());
        assert!(FieldType::U32.is_numeric());
        assert!(!FieldType::Bool.is_numeric());
        assert!(!FieldType::String.is_numeric());
        assert!(!FieldType::Enum(3).is_numeric());
    }

    #[test]
    fn priority_ordering() {
        assert!(Priority::Critical < Priority::High);
        assert!(Priority::High < Priority::Medium);
        assert!(Priority::Medium < Priority::Low);
    }

    #[test]
    fn schema_error_display() {
        let err = SchemaError::TypeNotFound("foo".into());
        assert_eq!(err.to_string(), "type not found: foo");
    }

    #[test]
    fn schema_warning_display() {
        let w = SchemaWarning::QuantizeWithoutClamp { field: "x".into() };
        assert!(w.to_string().contains("quantize without clamp"));
    }
}

/// Shared test fixtures for schema tests.
#[cfg(any(test, feature = "test-support"))]
pub mod test_fixtures {
    use super::*;

    pub fn minimal_schema() -> CompiledSchema {
        CompiledSchema {
            version: 1,
            fields: vec![FieldMeta {
                name: "alive".to_string(),
                field_type: FieldType::Bool,
                bit_width: 1,
                bit_offset: 0,
                group_index: 0,
                quantization: None,
                prediction: PredictionMode::None,
                smoothing: None,
                interpolation: InterpolationMode::None,
                skip_delta: false,
            }],
            field_groups: vec![FieldGroup {
                name: "default".to_string(),
                priority: Priority::Medium,
                max_tick_rate: 0,
                bitmask_range: (0, 1),
            }],
            total_bits: 1,
            bitmask_byte_count: 1,
        }
    }

    pub fn two_field_schema() -> CompiledSchema {
        CompiledSchema {
            version: 1,
            fields: vec![
                FieldMeta {
                    name: "alive".to_string(),
                    field_type: FieldType::Bool,
                    bit_width: 1,
                    bit_offset: 0,
                    group_index: 0,
                    quantization: None,
                    prediction: PredictionMode::None,
                    smoothing: None,
                    interpolation: InterpolationMode::None,
                    skip_delta: false,
                },
                FieldMeta {
                    name: "health".to_string(),
                    field_type: FieldType::U16,
                    bit_width: 16,
                    bit_offset: 1,
                    group_index: 0,
                    quantization: None,
                    prediction: PredictionMode::None,
                    smoothing: None,
                    interpolation: InterpolationMode::None,
                    skip_delta: false,
                },
            ],
            field_groups: vec![FieldGroup {
                name: "default".to_string(),
                priority: Priority::Medium,
                max_tick_rate: 0,
                bitmask_range: (0, 2),
            }],
            total_bits: 17,
            bitmask_byte_count: 1,
        }
    }

    pub fn schema_with_quantization_and_smoothing() -> CompiledSchema {
        CompiledSchema {
            version: 1,
            fields: vec![
                FieldMeta {
                    name: "x".to_string(),
                    field_type: FieldType::F32,
                    bit_width: 21,
                    bit_offset: 0,
                    group_index: 0,
                    quantization: Some(QuantizationParams {
                        min: -10000.0,
                        max: 10000.0,
                        precision: 0.01,
                        num_values: 2_000_001,
                        mask: (1u64 << 21) - 1,
                    }),
                    prediction: PredictionMode::InputReplay,
                    smoothing: Some(SmoothingParams {
                        mode: SmoothingMode::Lerp,
                        duration_ms: 100,
                        max_distance: 0.0,
                    }),
                    interpolation: InterpolationMode::Linear,
                    skip_delta: false,
                },
                FieldMeta {
                    name: "alive".to_string(),
                    field_type: FieldType::Bool,
                    bit_width: 1,
                    bit_offset: 21,
                    group_index: 0,
                    quantization: None,
                    prediction: PredictionMode::None,
                    smoothing: None,
                    interpolation: InterpolationMode::None,
                    skip_delta: false,
                },
            ],
            field_groups: vec![FieldGroup {
                name: "default".to_string(),
                priority: Priority::Medium,
                max_tick_rate: 0,
                bitmask_range: (0, 2),
            }],
            total_bits: 22,
            bitmask_byte_count: 1,
        }
    }
}

/// Wraps a NIF body in `catch_unwind` to prevent panics from crashing the BEAM.
///
/// On panic, encodes `{:error, "nif_panic: <message>"}` as a Term.
/// Requires `env: Env` in scope and `rustler::Encoder` imported.
///
/// # Usage
///
/// ```ignore
/// nif_safe!(env, {
///     let result = some_operation();
///     (atoms::ok(), result).encode(env)
/// })
/// ```
macro_rules! nif_safe {
    ($env:expr, $body:expr) => {{
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| $body)) {
            Ok(result) => result,
            Err(panic) => {
                let msg = if let Some(s) = panic.downcast_ref::<&str>() {
                    format!("nif_panic: {}", s)
                } else if let Some(s) = panic.downcast_ref::<String>() {
                    format!("nif_panic: {}", s)
                } else {
                    "nif_panic: unknown".to_string()
                };
                use rustler::Encoder;
                (rustler::types::atom::Atom::from_str($env, "error").unwrap(), msg).encode($env)
            }
        }
    }};
}

pub(crate) use nif_safe;

macro_rules! nif_safe {
    ($env:expr, $body:expr) => {{
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| $body)) {
            Ok(result) => result,
            Err(panic) => {
                let msg = $crate::macros::format_panic(panic);
                use rustler::Encoder;
                (rustler::types::atom::Atom::from_str($env, "error").unwrap(), msg).encode($env)
            }
        }
    }};
}

pub(crate) use nif_safe;

pub(crate) fn format_panic(panic: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        format!("nif_panic: {}", s)
    } else if let Some(s) = panic.downcast_ref::<String>() {
        format!("nif_panic: {}", s)
    } else {
        "nif_panic: unknown".to_string()
    }
}

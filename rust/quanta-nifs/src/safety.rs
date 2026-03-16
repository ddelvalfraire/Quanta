use std::any::Any;

pub fn extract_panic_message(panic: Box<dyn Any + Send>) -> String {
    if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = panic.downcast_ref::<&str>() {
        s.to_string()
    } else {
        "unknown panic".to_string()
    }
}

macro_rules! nif_safe {
    ($env:expr, $body:expr) => {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| $body)) {
            Ok(result) => result,
            Err(panic) => {
                let msg = $crate::safety::extract_panic_message(panic);
                let error_msg = format!("nif_panic: {}", msg);
                use rustler::Encoder;
                (rustler::types::atom::error(), error_msg).encode($env)
            }
        }
    };
}

pub(crate) use nif_safe;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_string_panic() {
        let panic: Box<dyn Any + Send> = Box::new(String::from("boom"));
        assert_eq!(extract_panic_message(panic), "boom");
    }

    #[test]
    fn extract_str_panic() {
        let panic: Box<dyn Any + Send> = Box::new("static boom");
        assert_eq!(extract_panic_message(panic), "static boom");
    }

    #[test]
    fn extract_unknown_panic() {
        let panic: Box<dyn Any + Send> = Box::new(42_i32);
        assert_eq!(extract_panic_message(panic), "unknown panic");
    }

    #[test]
    fn catch_unwind_catches_panic() {
        let result = std::panic::catch_unwind(|| {
            panic!("test panic");
        });
        let msg = extract_panic_message(result.unwrap_err());
        assert_eq!(msg, "test panic");
    }
}

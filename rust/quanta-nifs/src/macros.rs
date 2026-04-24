// This module intentionally left empty.
//
// The authoritative `nif_safe` macro lives in `crate::safety` (src/safety.rs).
// Historically this file contained a duplicate definition that used an unsafe
// pattern in the panic-recovery branch. That duplicate was removed (see C1
// regression test). Call sites must import from `crate::safety` — not
// `crate::macros`.
//
// This file is kept as an empty stub only because the C1 regression test at
// `tests/duplicate_macro_test.rs` uses `include_str!("../src/macros.rs")` to
// verify that no duplicate definition remains here.

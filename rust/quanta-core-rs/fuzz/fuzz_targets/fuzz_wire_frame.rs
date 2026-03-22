#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // decode_wire_frame must never panic on arbitrary input.
    let _ = quanta_core_rs::decode_wire_frame(data);
});

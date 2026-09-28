//! Any byte string: `read` and `peek` must return, never panic or blow up memory.
//! Run: `cargo +nightly fuzz run read -- -max_total_time=600 -rss_limit_mb=2048`
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = znimok_format::peek(data);
    if let Ok(doc) = znimok_format::read(data) {
        // Whatever reads must write and read back.
        let bytes = znimok_format::write(&doc, &znimok_format::WriteOptions::default());
        let again = znimok_format::read(&bytes).expect("re-read of a written document");
        assert_eq!(again.objects.len(), doc.objects.len());
    }
});

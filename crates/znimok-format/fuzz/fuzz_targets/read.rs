//! Any byte string: `read` and `peek` must return, never panic or blow up memory.
//! Run: `cargo +nightly fuzz run read -- -max_total_time=600 -rss_limit_mb=2048`
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = znimok_format::peek(data);
    // Video documents (ZK-96): the in-memory and the streaming reader agree, and a video that
    // reads writes and reads back with the same video blocks and stream.
    let streamed = znimok_format::read_from(
        &mut std::io::Cursor::new(data),
        &znimok_format::Limits::default(),
    );
    match znimok_format::read_any(data) {
        Ok(znimok_format::Loaded::Video(v)) => {
            assert!(streamed.is_ok(), "streaming reader refused a readable video");
            let mp4 = v.payload.bytes(data).expect("payload inside the input");
            let bytes = znimok_format::write_video(
                &v.doc,
                &v.video,
                &mp4,
                &znimok_format::WriteOptions::default(),
            );
            match znimok_format::read_any(&bytes).expect("re-read of a written video") {
                znimok_format::Loaded::Video(w) => assert_eq!(w.video, v.video),
                znimok_format::Loaded::Image(_) => panic!("a written video read as an image"),
            }
        }
        Ok(_) => assert!(streamed.is_ok()),
        Err(_) => assert!(streamed.is_err()),
    }
    if let Ok(doc) = znimok_format::read(data) {
        // Whatever reads must write and read back.
        let bytes = znimok_format::write(&doc, &znimok_format::WriteOptions::default());
        let again = znimok_format::read(&bytes).expect("re-read of a written document");
        assert_eq!(again.objects.len(), doc.objects.len());
    }
});

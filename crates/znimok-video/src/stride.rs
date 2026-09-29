//! Row pitch of video buffers (§7 items 5 and 6) — the most treacherous spot of the LH video.
//!
//! A decoder aligns BOTH width and height (a 1918×1050 recording came as a 1920×1056 buffer;
//! 1160 px → 4672 bytes per row). `MF_MT_DEFAULT_STRIDE` lies (says `w·4`), `IMF2DBuffer` that
//! would know is often missing, so the pitch is derived from the buffer length. A GPU staging
//! copy has `RowPitch ≠ w·4` too — copy by rows. RGB32 may be bottom-up (negative pitch).
//! Only relevant while a backend hands over CPU buffers without an explicit pitch (MF);
//! AVFoundation reports it.

/// The smallest pitch `≥ w·4` (in steps of 4, up to `w·4 + 4096`) that divides `len` exactly and
/// gives at least `h` rows; `fallback` when none does (`DeriveVideoStride`).
pub fn derive_stride(len: usize, w: u32, h: u32, fallback: usize) -> usize {
    if w == 0 || h == 0 || len == 0 {
        return fallback;
    }
    let min = w as usize * 4;
    (min..=min + 4096)
        .step_by(4)
        .find(|&s| len.is_multiple_of(s) && len / s >= h as usize)
        .unwrap_or(fallback)
}

/// Copy `h` rows of `row_bytes` from `src` (rows `src_pitch` apart; negative = bottom-up, the
/// first row is the LAST in memory) into a tight top-down buffer.
pub fn copy_rows(src: &[u8], src_pitch: isize, row_bytes: usize, h: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(row_bytes * h);
    let pitch = src_pitch.unsigned_abs();
    for y in 0..h {
        let r = if src_pitch >= 0 { y } else { h - 1 - y };
        let o = r * pitch;
        out.extend_from_slice(&src[o..o + row_bytes]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §7 item 5: 1918×1050 in a buffer aligned to 1920×1056 → pitch 7680, not 7672.
    #[test]
    fn stride_from_length_aligned_both_ways() {
        let len = 1920 * 4 * 1056;
        assert_eq!(len, 8_110_080);
        assert_eq!(derive_stride(len, 1918, 1050, 1918 * 4), 7680);
    }

    /// §7 item 5: 1160 px → 4672 bytes per row (aligned to 1168).
    #[test]
    fn stride_1160() {
        assert_eq!(derive_stride(4672 * 1088, 1160, 1080, 4640), 4672);
        assert_eq!(
            derive_stride(4640 * 1080, 1160, 1080, 0),
            4640,
            "tight buffer"
        );
    }

    /// No pitch fits → fallback; empty inputs → fallback.
    #[test]
    fn stride_fallback() {
        assert_eq!(derive_stride(7, 1, 1, 4), 4);
        assert_eq!(derive_stride(0, 10, 10, 40), 40);
        assert_eq!(derive_stride(100, 0, 10, 40), 40);
    }

    /// §7 item 6: rows are copied one by one when pitch ≠ w·4; bottom-up buffers flip.
    #[test]
    fn rows_with_padding_and_bottom_up() {
        // 2 px wide (8 bytes), pitch 12, 3 rows: row r filled with r+1, padding 0xEE
        let mut src = Vec::new();
        for r in 0..3u8 {
            src.extend_from_slice(&[r + 1; 8]);
            src.extend_from_slice(&[0xEE; 4]);
        }
        let top = copy_rows(&src, 12, 8, 3);
        assert_eq!(top.len(), 24);
        assert!(top[..8].iter().all(|&b| b == 1) && top[16..].iter().all(|&b| b == 3));
        let flipped = copy_rows(&src, -12, 8, 3);
        assert!(flipped[..8].iter().all(|&b| b == 3) && flipped[16..].iter().all(|&b| b == 1));
    }
}

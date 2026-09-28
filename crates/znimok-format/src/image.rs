//! PNG inside blocks: encoding the RGBA rasters and decoding them back under limits.

use std::io::Cursor;

use znimok_core::Raster;

use crate::{FormatError, Limits};

pub fn encode_png(r: &Raster, compression: png::Compression) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, r.width, r.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(compression);
        let mut w = enc.write_header().expect("in-memory PNG header");
        w.write_image_data(&r.rgba).expect("in-memory PNG data");
    }
    out
}

/// Decodes any 8/16-bit PNG to RGBA8. Size limits are checked from the header before any
/// pixel buffer is allocated.
pub fn decode_png(data: &[u8], limits: &Limits) -> Result<Raster, FormatError> {
    let bad = |e: png::DecodingError| FormatError::Corrupt(format!("PNG: {e}"));
    let mut dec = png::Decoder::new_with_limits(
        Cursor::new(data),
        png::Limits {
            bytes: limits.max_image_bytes,
        },
    );
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(bad)?;
    let (w, h) = reader.info().size();
    if w == 0
        || h == 0
        || w > limits.max_image_side
        || h > limits.max_image_side
        || (w as u64) * (h as u64) > limits.max_image_pixels
    {
        return Err(FormatError::Corrupt(format!(
            "image {w}×{h} exceeds the limits"
        )));
    }
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| FormatError::Corrupt("PNG size overflow".into()))?;
    // Deflate cannot compress better than about 1032:1, so a header announcing far more pixels
    // than the bytes can hold is a bomb: refuse before allocating the buffer.
    if size / 1032 > data.len() + 4096 {
        return Err(FormatError::Corrupt(format!(
            "image {w}×{h} cannot fit in {} bytes of PNG",
            data.len()
        )));
    }
    let mut buf = vec![0u8; size];
    let info = reader.next_frame(&mut buf).map_err(bad)?;
    buf.truncate(info.buffer_size());
    let px = (w * h) as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => {
            return Err(FormatError::Corrupt("PNG palette was not expanded".into()));
        }
    };
    if rgba.len() != px * 4 {
        return Err(FormatError::Corrupt(
            "PNG data is shorter than its header says".into(),
        ));
    }
    Ok(Raster::new(w, h, rgba))
}

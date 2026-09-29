//! QR codes and barcodes on a screenshot (ZK-119), read on the device — nothing goes to the
//! network. `rxing` (Apache-2.0) finds QR, Data Matrix, Aztec, PDF417 and the 1-D codes
//! (EAN, UPC, Code 128/39/93, ITF, Codabar). A second pass over the inverted picture catches light
//! codes on a dark background (dark themes). Each result says what it is, so the app can offer the
//! right action: a link opens only after the person saw the whole address (QR phishing is common),
//! Wi-Fi shows the network and the password without connecting.

use rxing::common::HybridBinarizer;
use rxing::multi::{GenericMultipleBarcodeReader, MultipleBarcodeReader};
use rxing::{BinaryBitmap, DecodeHints, Luma8LuminanceSource, MultiUseMultiFormatReader};

/// One code found on the picture.
#[derive(Clone, Debug, PartialEq)]
pub struct Code {
    /// "QR_CODE", "EAN_13", … as rxing names it.
    pub format: String,
    pub text: String,
    /// Where it is, in picture pixels: x, y, width, height (the box around the found points).
    pub bounds: (i32, i32, i32, i32),
    pub kind: Kind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// http or https — the only kind that may be opened, after a confirmation.
    Link(String),
    Wifi {
        ssid: String,
        password: Option<String>,
        security: String,
        hidden: bool,
    },
    /// vCard or MECARD: shown and copied, never imported silently.
    Contact,
    /// iCalendar event.
    Event,
    Email(String),
    Phone(String),
    Text,
}

/// Every code on an RGBA picture (straight alpha), each once, top to bottom.
pub fn read(width: u32, height: u32, rgba: &[u8]) -> Vec<Code> {
    if width == 0 || height == 0 || rgba.len() < (width as usize * height as usize * 4) {
        return Vec::new();
    }
    let luma: Vec<u8> = rgba
        .chunks_exact(4)
        .take(width as usize * height as usize)
        .map(|p| ((p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000) as u8)
        .collect();
    // A big screen is read at half size first (a code's modules on a screen are several pixels
    // wide, so it still decodes, four times faster: < 300 ms on 4K); full size only when that
    // found nothing — small or dense codes.
    let mut found = Vec::new();
    if width >= 1600 && height >= 900 {
        let (hw, hh, half) = halve(&luma, width, height);
        found = both_polarities(&half, hw, hh);
        for c in &mut found {
            let (x, y, w, h) = c.bounds;
            c.bounds = (x * 2, y * 2, w * 2, h * 2);
        }
    }
    if found.is_empty() {
        found = both_polarities(&luma, width, height);
    }
    found.sort_by_key(|c| (c.bounds.1, c.bounds.0));
    found
}

/// Dark on light, then light on dark (a dark theme) — the second only when the first found none.
fn both_polarities(luma: &[u8], width: u32, height: u32) -> Vec<Code> {
    let found = scan(luma, width, height);
    if !found.is_empty() {
        return found;
    }
    let inverted: Vec<u8> = luma.iter().map(|v| 255 - v).collect();
    scan(&inverted, width, height)
}

/// Half size, each pixel the mean of a 2×2 block.
fn halve(luma: &[u8], width: u32, height: u32) -> (u32, u32, Vec<u8>) {
    let (hw, hh) = (width / 2, height / 2);
    let w = width as usize;
    let mut out = Vec::with_capacity(hw as usize * hh as usize);
    for y in 0..hh as usize {
        let (r0, r1) = (2 * y * w, (2 * y + 1) * w);
        for x in 0..hw as usize {
            let s = luma[r0 + 2 * x] as u32
                + luma[r0 + 2 * x + 1] as u32
                + luma[r1 + 2 * x] as u32
                + luma[r1 + 2 * x + 1] as u32;
            out.push((s / 4) as u8);
        }
    }
    (hw, hh, out)
}

fn scan(luma: &[u8], width: u32, height: u32) -> Vec<Code> {
    let Ok(src) = Luma8LuminanceSource::new(luma.to_vec(), width, height) else {
        return Vec::new();
    };
    let mut bitmap = BinaryBitmap::new(HybridBinarizer::new(src));
    let mut reader = GenericMultipleBarcodeReader::new(MultiUseMultiFormatReader::default());
    let hints = DecodeHints {
        TryHarder: Some(true),
        ..Default::default()
    };
    let Ok(results) = reader.decode_multiple_with_hints(&mut bitmap, &hints) else {
        return Vec::new();
    };
    let mut out: Vec<Code> = Vec::new();
    for r in results {
        let text = r.getText().to_string();
        let format = format!("{:?}", r.getBarcodeFormat());
        if text.is_empty() || out.iter().any(|c| c.text == text && c.format == format) {
            continue;
        }
        let pts = r.getPoints();
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in pts {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        let bounds = if pts.is_empty() {
            (0, 0, 0, 0)
        } else {
            (
                x0 as i32,
                y0 as i32,
                (x1 - x0).max(1.0) as i32,
                (y1 - y0).max(1.0) as i32,
            )
        };
        let kind = classify(&text);
        out.push(Code {
            format,
            text,
            bounds,
            kind,
        });
    }
    out
}

/// What a code's text is.
pub fn classify(text: &str) -> Kind {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    if (lower.starts_with("https://") || lower.starts_with("http://"))
        && !t.contains(char::is_whitespace)
    {
        return Kind::Link(t.to_string());
    }
    if lower.starts_with("wifi:") {
        return wifi(&t[5..]);
    }
    if lower.starts_with("begin:vcard") || lower.starts_with("mecard:") {
        return Kind::Contact;
    }
    if lower.starts_with("begin:vcalendar") || lower.starts_with("begin:vevent") {
        return Kind::Event;
    }
    if let Some(rest) = lower.strip_prefix("mailto:") {
        return Kind::Email(t[t.len() - rest.len()..].to_string());
    }
    if let Some(rest) = lower.strip_prefix("tel:") {
        return Kind::Phone(t[t.len() - rest.len()..].to_string());
    }
    Kind::Text
}

/// `WIFI:T:WPA;S:name;P:secret;H:false;;` — fields in any order, `\` escapes `;,:\"`.
fn wifi(body: &str) -> Kind {
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut cur = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            ';' => {
                if let Some((k, v)) = cur.split_once(':') {
                    fields.push((k.to_ascii_uppercase(), v.to_string()));
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if let Some((k, v)) = cur.split_once(':') {
        fields.push((k.to_ascii_uppercase(), v.to_string()));
    }
    let get = |k: &str| fields.iter().find(|(f, _)| f == k).map(|(_, v)| v.clone());
    Kind::Wifi {
        ssid: get("S").unwrap_or_default(),
        password: get("P").filter(|p| !p.is_empty()),
        security: get("T").unwrap_or_else(|| "nopass".into()),
        hidden: get("H").is_some_and(|h| h.eq_ignore_ascii_case("true")),
    }
}

/// `text` as a QR code: dark modules on white, `px` pixels a module, a 4-module quiet zone.
/// Straight RGBA.
#[cfg(feature = "encode")]
pub fn qr_rgba(text: &str, px: u32) -> Option<(u32, u32, Vec<u8>)> {
    use rxing::Writer;
    let m = rxing::MultiFormatWriter
        .encode(text, &rxing::BarcodeFormat::QR_CODE, 0, 0)
        .ok()?;
    let (mw, mh) = (m.getWidth(), m.getHeight());
    let (w, h) = ((mw + 8) * px, (mh + 8) * px);
    let mut out = vec![255u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let (mx, my) = (x / px, y / px);
            if mx >= 4 && my >= 4 && mx < mw + 4 && my < mh + 4 && m.get(mx - 4, my - 4) {
                let i = ((y * w + x) * 4) as usize;
                out[i..i + 3].copy_from_slice(&[20, 20, 24]);
            }
        }
    }
    Some((w, h, out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rxing::{BarcodeFormat, MultiFormatWriter, Writer};

    /// A picture `w`×`h` of light grey "UI" with a QR of `text` at (x, y), `px` pixels a module.
    fn scene(w: u32, h: u32, codes: &[(&str, u32, u32, u32)], dark: bool) -> Vec<u8> {
        let (bg, fg) = if dark { (30u8, 230u8) } else { (245u8, 20u8) };
        let mut img = vec![0u8; (w * h * 4) as usize];
        for (i, p) in img.chunks_exact_mut(4).enumerate() {
            let y = i as u32 / w;
            // Stripes of "text lines" to make the background less clean.
            let v = if y % 40 < 6 {
                bg.saturating_sub(40).max(bg.min(40))
            } else {
                bg
            };
            p.copy_from_slice(&[v, v, v, 255]);
        }
        for (text, x, y, px) in codes {
            let m = MultiFormatWriter
                .encode(text, &BarcodeFormat::QR_CODE, 0, 0)
                .expect("encodes");
            let (mw, mh) = (m.getWidth(), m.getHeight());
            // Quiet zone of 4 modules, then the modules.
            for my in 0..mh + 8 {
                for mx in 0..mw + 8 {
                    let on =
                        mx >= 4 && my >= 4 && mx < mw + 4 && my < mh + 4 && m.get(mx - 4, my - 4);
                    let v = if on { fg } else { bg };
                    for dy in 0..*px {
                        for dx in 0..*px {
                            let (xx, yy) = (x + mx * px + dx, y + my * px + dy);
                            if xx < w && yy < h {
                                let i = ((yy * w + xx) * 4) as usize;
                                img[i..i + 3].copy_from_slice(&[v, v, v]);
                            }
                        }
                    }
                }
            }
        }
        img
    }

    #[test]
    fn finds_codes_on_light_and_dark_screens() {
        let img = scene(
            1200,
            800,
            &[
                ("https://example.org/znimok?q=1", 80, 120, 5),
                ("WIFI:T:WPA;S:Дім;P:pa\\;ss;;", 700, 300, 4),
            ],
            false,
        );
        let got = read(1200, 800, &img);
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(
            got[0].kind,
            Kind::Link("https://example.org/znimok?q=1".into())
        );
        assert_eq!(
            got[1].kind,
            Kind::Wifi {
                ssid: "Дім".into(),
                password: Some("pa;ss".into()),
                security: "WPA".into(),
                hidden: false
            }
        );
        assert!(
            got[0].bounds.0 >= 80 && got[0].bounds.0 < 200,
            "{:?}",
            got[0].bounds
        );
        // Light on dark (a dark theme): found by the inverted pass.
        let dark = scene(900, 600, &[("hello", 300, 200, 6)], true);
        let got = read(900, 600, &dark);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].kind, Kind::Text);
    }

    #[test]
    fn nothing_on_a_plain_picture() {
        let img = scene(640, 480, &[], false);
        assert!(read(640, 480, &img).is_empty());
        assert!(read(0, 0, &[]).is_empty());
    }

    #[test]
    fn classifies() {
        assert_eq!(classify("http://a.b"), Kind::Link("http://a.b".into()));
        assert_eq!(classify("javascript:alert(1)"), Kind::Text);
        assert_eq!(classify("mailto:a@b.c"), Kind::Email("a@b.c".into()));
        assert_eq!(
            classify("tel:+380501234567"),
            Kind::Phone("+380501234567".into())
        );
        assert_eq!(classify("BEGIN:VCARD\nFN:X\nEND:VCARD"), Kind::Contact);
        assert!(matches!(
            classify("WIFI:S:x;;"),
            Kind::Wifi { password: None, .. }
        ));
    }

    #[test]
    fn small_code_on_a_big_screen_falls_back_to_full_size() {
        // Modules of 2 px: too small at half size.
        let img = scene(
            1920,
            1080,
            &[("https://example.org/small", 900, 500, 2)],
            false,
        );
        let got = read(1920, 1080, &img);
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(
            got[0].bounds.0 >= 900 && got[0].bounds.0 < 960,
            "{:?}",
            got[0].bounds
        );
    }

    /// The ticket's bar: < 300 ms on a 4K screen (release build; debug is much slower).
    #[test]
    #[ignore]
    fn speed_on_4k() {
        let img = scene(
            3840,
            2160,
            &[("https://example.org/x", 2000, 1200, 6)],
            false,
        );
        let t = std::time::Instant::now();
        let got = read(3840, 2160, &img);
        eprintln!("4K: {:?}, {} codes", t.elapsed(), got.len());
        assert_eq!(got.len(), 1);
        assert!(
            got[0].bounds.0 >= 2000 && got[0].bounds.0 < 2100,
            "{:?}",
            got[0].bounds
        );
        let t = std::time::Instant::now();
        let none = read(3840, 2160, &scene(3840, 2160, &[], false));
        eprintln!("4K, no code: {:?}", t.elapsed());
        assert!(none.is_empty());
    }
}

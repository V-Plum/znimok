//! A ZIP archive without compression (the video inside is compressed already; the JSON is small
//! next to it): written in one pass, read back by its central directory. Only what a `.zreport`
//! needs — no ZIP64, no encryption, names in UTF-8.

use std::io::Write;

/// The files as `(name, bytes)` into `out`.
pub fn write(out: &mut impl Write, files: &[(&str, &[u8])]) -> std::io::Result<()> {
    let mut central = Vec::new();
    let mut at: u64 = 0;
    let too_big = || std::io::Error::other("a .zreport over 4 GB");
    for (name, data) in files {
        let crc = crc32fast::hash(data);
        let size = u32::try_from(data.len()).map_err(|_| too_big())?;
        let offset = u32::try_from(at).map_err(|_| too_big())?;
        let name = name.as_bytes();
        let mut local = Vec::with_capacity(30 + name.len());
        local.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        local.extend_from_slice(&20u16.to_le_bytes()); // version needed
        local.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
        local.extend_from_slice(&0u16.to_le_bytes()); // stored
        local.extend_from_slice(&0u32.to_le_bytes()); // time, date
        local.extend_from_slice(&crc.to_le_bytes());
        local.extend_from_slice(&size.to_le_bytes());
        local.extend_from_slice(&size.to_le_bytes());
        local.extend_from_slice(&(name.len() as u16).to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(name);
        out.write_all(&local)?;
        out.write_all(data)?;
        at += (local.len() + data.len()) as u64;
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // made by
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0x0800u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0u8; 12]); // extra, comment, disk, attributes
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let start = u32::try_from(at).map_err(|_| too_big())?;
    out.write_all(&central)?;
    let mut end = Vec::with_capacity(22);
    end.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    end.extend_from_slice(&[0u8; 4]);
    end.extend_from_slice(&(files.len() as u16).to_le_bytes());
    end.extend_from_slice(&(files.len() as u16).to_le_bytes());
    end.extend_from_slice(&(central.len() as u32).to_le_bytes());
    end.extend_from_slice(&start.to_le_bytes());
    end.extend_from_slice(&0u16.to_le_bytes());
    out.write_all(&end)
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The stored files of an archive, by its central directory; each checked against its CRC.
pub fn read(b: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let bad = |what: &str| format!("not a .zreport ({what})");
    // The end record: within the last 64 KB + 22 bytes (a comment may follow it).
    let from = b.len().saturating_sub(22 + 65_535);
    let end = (from..=b.len().saturating_sub(22))
        .rev()
        .find(|&i| u32_at(b, i) == Some(0x0605_4b50))
        .ok_or_else(|| bad("no end record"))?;
    let count = u16_at(b, end + 10).ok_or_else(|| bad("end"))? as usize;
    let mut at = u32_at(b, end + 16).ok_or_else(|| bad("end"))? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if u32_at(b, at) != Some(0x0201_4b50) {
            return Err(bad("directory"));
        }
        let method = u16_at(b, at + 10).ok_or_else(|| bad("directory"))?;
        let crc = u32_at(b, at + 16).ok_or_else(|| bad("directory"))?;
        let size = u32_at(b, at + 20).ok_or_else(|| bad("directory"))? as usize;
        let name_len = u16_at(b, at + 28).ok_or_else(|| bad("directory"))? as usize;
        let extra = u16_at(b, at + 30).ok_or_else(|| bad("directory"))? as usize;
        let comment = u16_at(b, at + 32).ok_or_else(|| bad("directory"))? as usize;
        let local = u32_at(b, at + 42).ok_or_else(|| bad("directory"))? as usize;
        let name = b
            .get(at + 46..at + 46 + name_len)
            .ok_or_else(|| bad("name"))?;
        let name = String::from_utf8_lossy(name).into_owned();
        if method != 0 {
            return Err(format!("{name}: compressed entries are not read"));
        }
        if u32_at(b, local) != Some(0x0403_4b50) {
            return Err(bad("entry"));
        }
        let lname = u16_at(b, local + 26).ok_or_else(|| bad("entry"))? as usize;
        let lextra = u16_at(b, local + 28).ok_or_else(|| bad("entry"))? as usize;
        let start = local + 30 + lname + lextra;
        let data = b.get(start..start + size).ok_or_else(|| bad("data"))?;
        if crc32fast::hash(data) != crc {
            return Err(format!("{name}: damaged (CRC)"));
        }
        out.push((name, data.to_vec()));
        at += 46 + name_len + extra + comment;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trip() {
        let mut buf = Vec::new();
        let video: Vec<u8> = (0..100_000u32).map(|i| (i * 7) as u8).collect();
        super::write(
            &mut buf,
            &[
                ("report.html", b"<!doctype html>"),
                ("video.mp4", &video),
                ("лог.json", b"[]"),
            ],
        )
        .unwrap();
        let files = super::read(&buf).unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(files[1].0, "video.mp4");
        assert_eq!(files[1].1, video);
        assert_eq!(files[2].0, "лог.json");
        // A damaged byte is caught.
        let at = buf.windows(4).position(|w| w == [0, 7, 14, 21]).unwrap();
        buf[at + 1] ^= 1;
        assert!(super::read(&buf).unwrap_err().contains("CRC"));
    }
}

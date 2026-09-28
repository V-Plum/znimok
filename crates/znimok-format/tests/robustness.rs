//! ZK-27, the part that runs in every CI build: thousands of damaged variants of a real
//! document must be rejected cleanly — no panic, no runaway allocation, bounded time. The
//! open-ended search runs under cargo-fuzz (`crates/znimok-format/fuzz`).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use znimok_format::{FormatError, WriteOptions, peek, read, write};

/// Small deterministic generator (xorshift64*), so failures reproduce from the seed.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn sample() -> Vec<u8> {
    let doc = znimok_render::reference::reference_document(160, 100);
    let thumb = znimok_core::Raster::solid(16, 10, znimok_core::Rgb::BLUE);
    write(
        &doc,
        &WriteOptions {
            thumbnail: Some(thumb),
            ..Default::default()
        },
    )
}

fn mutate(rng: &mut Rng, base: &[u8]) -> Vec<u8> {
    let mut v = base.to_vec();
    for _ in 0..=rng.below(4) {
        match rng.below(7) {
            0 => {
                let i = rng.below(v.len());
                v[i] ^= 1 << rng.below(8);
            }
            1 => {
                let i = rng.below(v.len());
                v[i] = rng.next() as u8;
            }
            2 => v.truncate(rng.below(v.len())),
            3 => {
                let i = rng.below(v.len());
                let n = rng.below(64);
                let junk: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
                v.splice(i..i, junk);
            }
            4 => {
                // Overwrite a 32-bit field with a huge or boundary value (lengths, counts).
                if v.len() > 16 {
                    let i = 12 + rng.below(v.len() - 16);
                    let val: u32 =
                        [u32::MAX, 0x7FFF_FFFF, 0x8000_0000, 1 << 20, 0, 65535][rng.below(6)];
                    v[i..i + 4].copy_from_slice(&val.to_le_bytes());
                }
            }
            5 => {
                // Duplicate a slice somewhere else.
                let a = rng.below(v.len());
                let n = rng.below(256).min(v.len() - a);
                let chunk = v[a..a + n].to_vec();
                let at = rng.below(v.len());
                v.splice(at..at, chunk);
            }
            _ => {
                let a = rng.below(v.len());
                let b = (a + rng.below(128)).min(v.len());
                v.drain(a..b);
            }
        }
        if v.len() < 12 {
            break;
        }
    }
    v
}

#[test]
fn damaged_files_never_panic() {
    let base = sample();
    assert!(read(&base).is_ok());
    let mut rng = Rng(0x5EED_2026_0928);
    let started = Instant::now();
    let mut outcomes = [0usize; 3];
    for i in 0..4000 {
        let v = mutate(&mut rng, &base);
        let t = Instant::now();
        let r = catch_unwind(AssertUnwindSafe(|| (read(&v), peek(&v))));
        let (full, _) = r.unwrap_or_else(|_| panic!("panic on case {i} (seed 0x5EED_2026_0928)"));
        assert!(
            t.elapsed() < Duration::from_secs(2),
            "case {i} took {:?}",
            t.elapsed()
        );
        match full {
            Ok(_) => outcomes[0] += 1,
            Err(FormatError::Corrupt(_)) => outcomes[1] += 1,
            Err(_) => outcomes[2] += 1,
        }
    }
    eprintln!(
        "4000 damaged files in {:?}: {} still readable, {} corrupt, {} other",
        started.elapsed(),
        outcomes[0],
        outcomes[1],
        outcomes[2]
    );
    assert!(outcomes[1] > 1000, "most damage must be detected");
}

#[test]
fn length_bombs_fail_fast_without_allocating() {
    use znimok_format::MAGIC;
    let frame = |blocks: &[(&[u8; 4], Vec<u8>)]| {
        let mut v = MAGIC.to_vec();
        v.extend_from_slice(&[1, 0, 0, 0]);
        for (t, b) in blocks {
            v.extend_from_slice(*t);
            v.extend_from_slice(&(b.len() as u32).to_le_bytes());
            v.extend_from_slice(b);
        }
        v
    };
    // Object count and pen point count far beyond the limits.
    let objs = frame(&[(b"OBJS", u32::MAX.to_le_bytes().to_vec())]);
    assert!(matches!(read(&objs), Err(FormatError::Corrupt(m)) if m.contains("limit")));
    // A block claiming more bytes than the file has.
    let mut short = frame(&[]);
    short.extend_from_slice(b"SRC ");
    short.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(read(&short), Err(FormatError::Corrupt(_))));
    // A PNG header announcing 30000×30000 pixels: refused from the header.
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&30000u32.to_be_bytes());
    ihdr.extend_from_slice(&30000u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    png.extend_from_slice(&(ihdr.len() as u32).to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&ihdr);
    let mut crc_in = b"IHDR".to_vec();
    crc_in.extend_from_slice(&ihdr);
    png.extend_from_slice(&crc32(&crc_in).to_be_bytes());
    let t = Instant::now();
    let big = frame(&[(b"SRC ", png)]);
    assert!(matches!(read(&big), Err(FormatError::Corrupt(_))));
    assert!(t.elapsed() < Duration::from_millis(500));
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// Writes the seed corpus for cargo-fuzz when `ZNIMOK_FUZZ_SEED=<dir>` is set.
#[test]
fn write_fuzz_seed_if_asked() {
    if let Ok(dir) = std::env::var("ZNIMOK_FUZZ_SEED") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("reference.znimok"),
            sample(),
        )
        .unwrap();
        let small = znimok_core::Document::from_raster(
            "s",
            znimok_core::Raster::solid(3, 2, znimok_core::Rgb::RED),
        );
        std::fs::write(
            std::path::Path::new(&dir).join("small.znimok"),
            write(&small, &WriteOptions::default()),
        )
        .unwrap();
    }
}

/// Inputs that once crashed the reader (found by cargo-fuzz). Each must now read or fail
/// cleanly — and whatever reads must survive a write/read round trip.
#[test]
fn fuzz_regressions() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fuzz-regressions");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap() {
        let data = std::fs::read(e.unwrap().path()).unwrap();
        let _ = peek(&data);
        if let Ok(doc) = read(&data) {
            for o in &doc.objects {
                let _ = o.bounds();
            }
            let again = read(&write(&doc, &WriteOptions::default())).unwrap();
            assert_eq!(again.objects.len(), doc.objects.len());
        }
        n += 1;
    }
    assert!(n >= 1);
}

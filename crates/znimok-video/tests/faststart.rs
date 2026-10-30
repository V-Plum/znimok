//! `moov` before `mdat` (ZK-345): the rearranged file keeps every chunk on the same bytes,
//! parses as before, widens to `co64` when asked, and is left alone when already in order.

use std::io::Read;
use znimok_video::check::mp4::parse_mp4;
use znimok_video::faststart::FastStart;

fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut v = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(body);
    v
}

fn fullbox(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut b = vec![0u8; 4];
    b.extend_from_slice(body);
    bx(kind, &b)
}

fn be32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

/// A track of `n` samples of `size` bytes, one per chunk, at `offsets`; `wide` writes `co64`.
fn trak(handler: &[u8; 4], id: u32, n: u32, size: u32, offsets: &[u64], wide: bool) -> Vec<u8> {
    let mut b = Vec::new();
    be32(&mut b, 0);
    be32(&mut b, 0);
    be32(&mut b, id);
    be32(&mut b, 0);
    be32(&mut b, n);
    b.extend_from_slice(&[0; 8 + 2 + 2 + 2 + 2 + 36]);
    be32(&mut b, 16 << 16);
    be32(&mut b, 16 << 16);
    let tkhd = fullbox(b"tkhd", &b);
    let mut b = Vec::new();
    be32(&mut b, 0);
    be32(&mut b, 0);
    be32(&mut b, 30);
    be32(&mut b, n);
    be32(&mut b, 0);
    let mdhd = fullbox(b"mdhd", &b);
    let mut b = vec![0; 4];
    b.extend_from_slice(handler);
    b.extend_from_slice(&[0; 13]);
    let hdlr = fullbox(b"hdlr", &b);
    let mut e = vec![0u8; 78];
    e[24..26].copy_from_slice(&16u16.to_be_bytes());
    e[26..28].copy_from_slice(&16u16.to_be_bytes());
    let mut b = Vec::new();
    be32(&mut b, 1);
    b.extend(bx(b"avc1", &e));
    let stsd = fullbox(b"stsd", &b);
    let mut b = Vec::new();
    be32(&mut b, 1);
    be32(&mut b, n);
    be32(&mut b, 1);
    let stts = fullbox(b"stts", &b);
    let mut b = Vec::new();
    be32(&mut b, size);
    be32(&mut b, n);
    let stsz = fullbox(b"stsz", &b);
    let mut b = Vec::new();
    be32(&mut b, 1);
    be32(&mut b, 1);
    be32(&mut b, 1);
    be32(&mut b, 1);
    let stsc = fullbox(b"stsc", &b);
    let mut b = Vec::new();
    be32(&mut b, n);
    for &o in offsets {
        if wide {
            b.extend_from_slice(&o.to_be_bytes());
        } else {
            be32(&mut b, o as u32);
        }
    }
    let stco = fullbox(if wide { b"co64" } else { b"stco" }, &b);
    let stbl = bx(b"stbl", &[stsd, stts, stsz, stsc, stco].concat());
    let mdia = bx(b"mdia", &[mdhd, hdlr, bx(b"minf", &stbl)].concat());
    bx(b"trak", &[tkhd, mdia].concat())
}

/// `ftyp`, a `free` (as MF's `uuid`), `mdat` with the chunks of two tracks (video `stco`, audio
/// `co64`), `moov` last — as the writers leave it. Chunk `i` of track `t` is filled with
/// `t * 16 + i`.
fn moov_last(video_n: u32, audio_n: u32, size: u32) -> Vec<u8> {
    let ftyp = bx(b"ftyp", b"isom\0\0\0\0isomavc1");
    let free = bx(b"free", &[0u8; 23]);
    let mut data = Vec::new();
    let mut offs = [Vec::new(), Vec::new()];
    let mdat_at = (ftyp.len() + free.len() + 8) as u64;
    for (t, n) in [(0u8, video_n), (1u8, audio_n)] {
        for i in 0..n {
            offs[t as usize].push(mdat_at + data.len() as u64);
            data.extend(std::iter::repeat_n(t * 16 + i as u8, size as usize));
        }
    }
    let mdat = bx(b"mdat", &data);
    let mut b = Vec::new();
    be32(&mut b, 0);
    be32(&mut b, 0);
    be32(&mut b, 30);
    be32(&mut b, video_n.max(audio_n));
    b.extend_from_slice(&[0; 80]);
    let mvhd = fullbox(b"mvhd", &b);
    let v = trak(b"vide", 1, video_n, size, &offs[0], false);
    let a = trak(b"soun", 2, audio_n, size, &offs[1], true);
    let moov = bx(b"moov", &[mvhd, v, a].concat());
    [ftyp, free, mdat, moov].concat()
}

fn rearranged(bytes: &[u8], wide: bool) -> (Vec<u8>, bool, u64) {
    let mut fs = FastStart::open_with(std::io::Cursor::new(bytes), wide).unwrap();
    let (changed, len) = (fs.changed(), fs.len());
    let mut out = Vec::new();
    fs.read_to_end(&mut out).unwrap();
    assert_eq!(out.len() as u64, len, "the announced length");
    (out, changed, len)
}

/// Every chunk still points at its own bytes.
fn chunks_hold(bytes: &[u8]) {
    let info = parse_mp4(bytes).unwrap();
    for (t, track) in info.tracks.iter().enumerate() {
        for (i, (off, n, _)) in track.chunk_layout().unwrap().into_iter().enumerate() {
            let want = (t * 16 + i) as u8;
            let got = &bytes[off as usize..(off + n) as usize];
            assert!(
                got.iter().all(|&b| b == want),
                "track {t} chunk {i} at {off}: {:?}",
                &got[..4]
            );
        }
    }
}

#[test]
fn moov_moves_before_mdat_and_chunks_follow() {
    let src = moov_last(5, 3, 40);
    chunks_hold(&src);
    assert!(parse_mp4(&src).unwrap().moov_last());

    let (out, changed, len) = rearranged(&src, false);
    assert!(changed);
    assert_eq!(len, src.len() as u64, "nothing widened: the same length");
    let info = parse_mp4(&out).unwrap();
    let order: Vec<&[u8]> = info.top.iter().map(|b| &b.kind[..]).collect();
    assert_eq!(order, [b"ftyp", b"free", b"moov", b"mdat"]);
    assert!(!info.moov_last());
    chunks_hold(&out);
    // The video keeps `stco`, the audio its `co64`.
    assert!(out.windows(4).any(|w| w == b"stco") && out.windows(4).any(|w| w == b"co64"));
    // The checks of the container are as happy as before.
    let before =
        znimok_video::check::mp4::check_mp4(&parse_mp4(&src).unwrap(), &Default::default());
    let after = znimok_video::check::mp4::check_mp4(&info, &Default::default());
    assert_eq!(
        before.iter().map(|c| (c.name, c.ok)).collect::<Vec<_>>(),
        after.iter().map(|c| (c.name, c.ok)).collect::<Vec<_>>()
    );
}

#[test]
fn already_in_order_passes_through() {
    let (out, _, _) = rearranged(&moov_last(4, 2, 16), false);
    let (again, changed, len) = rearranged(&out, false);
    assert!(!changed);
    assert_eq!(len, out.len() as u64);
    assert_eq!(again, out);
    // No mdat at all: nothing to move either.
    let bare = [bx(b"ftyp", b"isom"), bx(b"moov", &[])].concat();
    let (same, changed, _) = rearranged(&bare, false);
    assert!(!changed && same == bare);
}

#[test]
fn widening_to_co64_keeps_the_chunks() {
    let src = moov_last(6, 2, 24);
    let (out, changed, len) = rearranged(&src, true);
    assert!(changed);
    assert_eq!(len, src.len() as u64 + 4 * 6, "4 bytes per widened entry");
    assert!(!out.windows(4).any(|w| w == b"stco"));
    let info = parse_mp4(&out).unwrap();
    assert!(!info.moov_last());
    chunks_hold(&out);
}

#[test]
fn reads_in_small_pieces() {
    let src = moov_last(3, 3, 50);
    let (whole, _, _) = rearranged(&src, false);
    let mut fs = FastStart::open(std::io::Cursor::new(&src)).unwrap();
    let mut out = Vec::new();
    let mut buf = [0u8; 7];
    loop {
        let n = fs.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
    }
    assert_eq!(out, whole);
}

#[test]
fn rewrite_file_in_place() {
    let dir = std::env::temp_dir().join(format!("znimok-faststart-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rec.mp4.part");
    let src = moov_last(4, 4, 32);
    std::fs::write(&path, &src).unwrap();
    assert!(znimok_video::faststart::rewrite_file(&path).unwrap());
    let out = std::fs::read(&path).unwrap();
    assert!(!parse_mp4(&out).unwrap().moov_last());
    chunks_hold(&out);
    assert!(!dir.join("rec.mp4.fsx").exists());
    assert!(!znimok_video::faststart::rewrite_file(&path).unwrap());
    assert_eq!(std::fs::read(&path).unwrap(), out);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hostile_input_is_an_error_not_a_panic() {
    // A moov whose inner box claims more than there is.
    let mut src = moov_last(2, 1, 8);
    let moov_at = src.len() - parse_mp4(&src).unwrap().top.last().unwrap().size as usize;
    src[moov_at + 8..moov_at + 12].copy_from_slice(&0xFFFF_FFFFu32.to_be_bytes());
    assert!(FastStart::open(std::io::Cursor::new(&src)).is_err());
    // A truncated file.
    let cut = &moov_last(2, 1, 8)[..40];
    assert!(FastStart::open(std::io::Cursor::new(cut)).is_err());
}

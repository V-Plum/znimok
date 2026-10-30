//! `moov` before `mdat` ("faststart", ZK-345): a player streaming the file from a site can start
//! as soon as the sample tables arrive, instead of after the whole `mdat`. Both writers (MF's
//! sink writer and AVAssetWriter) put `moov` last, so the file is rearranged after them.
//!
//! The rearrangement is a *view*: [`FastStart`] reads the source in its new order — the boxes
//! before the first `mdat` as they are, then the patched `moov`, then the rest — so embedding a
//! recording into a `.znimok` (which copies the bytes anyway) costs nothing extra, and an export
//! is rewritten once through [`rewrite_file`]. Only `moov` is held in memory (capped at
//! [`MAX_MOOV`]); `mdat` is streamed.
//!
//! Patching `moov`: every chunk offset (`stco` / `co64`) is moved by the shift of the top-level
//! box it points into. When the moved offsets would not fit 32 bits (a file over 4 GiB), every
//! `stco` becomes a `co64`, which grows `moov` by 4 bytes per entry — the growth itself is part
//! of the shift, so the length is settled first and the offsets after it. Everything else in
//! `moov` is copied byte for byte.

use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Largest `moov` rearranged (as the reader's cap: an hour at 60 fps with audio is ~5 MB).
pub const MAX_MOOV: u64 = crate::check::mp4::MAX_MOOV;
/// Top-level boxes before giving up (a real file has 3–6).
const MAX_TOP: usize = 4096;
/// Children of one container box inside `moov`.
const MAX_CHILDREN: usize = 1 << 16;
/// Containers walked on the way to the chunk offsets.
const CONTAINERS: [&[u8; 4]; 5] = [b"moov", b"trak", b"mdia", b"minf", b"stbl"];

fn bad(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

/// A top-level box: where it is in the source.
#[derive(Clone, Copy, Debug)]
struct Top {
    kind: [u8; 4],
    offset: u64,
    /// Header included.
    size: u64,
    header: u64,
}

/// Scans the top-level boxes of a seekable stream; returns them and the stream length.
fn scan<R: Read + Seek>(r: &mut R) -> io::Result<(Vec<Top>, u64)> {
    let len = r.seek(SeekFrom::End(0))?;
    let mut top = Vec::new();
    let mut off = 0u64;
    while off < len {
        if top.len() >= MAX_TOP {
            return Err(bad("too many top-level boxes"));
        }
        if len - off < 8 {
            return Err(bad("truncated box header"));
        }
        r.seek(SeekFrom::Start(off))?;
        let mut h = [0u8; 8];
        r.read_exact(&mut h)?;
        let size32 = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as u64;
        let kind = [h[4], h[5], h[6], h[7]];
        let (size, header) = match size32 {
            1 => {
                if len - off < 16 {
                    return Err(bad("truncated 64-bit box size"));
                }
                let mut l = [0u8; 8];
                r.read_exact(&mut l)?;
                (u64::from_be_bytes(l), 16)
            }
            0 => (len - off, 8),
            s => (s, 8),
        };
        if size < header || size > len - off {
            return Err(bad(format!(
                "box {} of {size} bytes does not fit",
                fourcc(&kind)
            )));
        }
        top.push(Top {
            kind,
            offset: off,
            size,
            header,
        });
        off += size;
    }
    Ok((top, len))
}

fn fourcc(k: &[u8; 4]) -> String {
    k.iter()
        .map(|&b| if b.is_ascii_graphic() { b as char } else { '?' })
        .collect()
}

/// One box inside `moov`: kind and payload range.
struct Child {
    kind: [u8; 4],
    a: usize,
    z: usize,
}

fn children(b: &[u8]) -> io::Result<Vec<Child>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < b.len() {
        if out.len() >= MAX_CHILDREN {
            return Err(bad("too many boxes inside moov"));
        }
        if b.len() - pos < 8 {
            return Err(bad("truncated box inside moov"));
        }
        let size32 = u32::from_be_bytes([b[pos], b[pos + 1], b[pos + 2], b[pos + 3]]) as usize;
        let kind = [b[pos + 4], b[pos + 5], b[pos + 6], b[pos + 7]];
        let (size, header) = match size32 {
            1 => {
                if b.len() - pos < 16 {
                    return Err(bad("truncated 64-bit box size inside moov"));
                }
                let l: [u8; 8] = b[pos + 8..pos + 16].try_into().unwrap();
                let s = u64::from_be_bytes(l);
                if s > (b.len() - pos) as u64 {
                    return Err(bad("box inside moov does not fit"));
                }
                (s as usize, 16)
            }
            0 => (b.len() - pos, 8),
            s => (s, 8),
        };
        if size < header || size > b.len() - pos {
            return Err(bad(format!(
                "box {} inside moov does not fit",
                fourcc(&kind)
            )));
        }
        out.push(Child {
            kind,
            a: pos + header,
            z: pos + size,
        });
        pos += size;
    }
    Ok(out)
}

/// How a chunk offset moves: the shift of the top-level box it points into.
struct Shift<'a> {
    top: &'a [Top],
    delta: &'a [i64],
}

impl Shift<'_> {
    fn map(&self, o: u64) -> u64 {
        let i = self.top.partition_point(|b| b.offset <= o);
        if i == 0 {
            return o;
        }
        let b = self.top[i - 1];
        if o < b.offset + b.size {
            (o as i64 + self.delta[i - 1]) as u64
        } else {
            o
        }
    }
}

/// Rebuilds a container's payload: containers recurse, chunk offsets are mapped (and widened to
/// `co64` when `wide`), everything else is copied. Counts the `stco` entries seen in `stco_n`.
fn rebuild(b: &[u8], shift: &Shift, wide: bool, stco_n: &mut u64) -> io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(b.len());
    for Child { kind, a, z } in children(b)? {
        let body = &b[a..z];
        if CONTAINERS[1..].contains(&&kind) {
            let inner = rebuild(body, shift, wide, stco_n)?;
            push_box(&mut out, &kind, &inner);
        } else if &kind == b"stco" || &kind == b"co64" {
            let is_wide = &kind == b"co64";
            if body.len() < 8 {
                return Err(bad("truncated stco"));
            }
            let n = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
            let w = if is_wide { 8 } else { 4 };
            if body.len() - 8 < n * w {
                return Err(bad("stco shorter than its count"));
            }
            if !is_wide {
                *stco_n += n as u64;
            }
            let to_wide = is_wide || wide;
            let mut nb = Vec::with_capacity(8 + n * if to_wide { 8 } else { 4 });
            nb.extend_from_slice(&body[..8]);
            for i in 0..n {
                let e = &body[8 + i * w..8 + (i + 1) * w];
                let o = if is_wide {
                    u64::from_be_bytes(e.try_into().unwrap())
                } else {
                    u32::from_be_bytes(e.try_into().unwrap()) as u64
                };
                let m = shift.map(o);
                if to_wide {
                    nb.extend_from_slice(&m.to_be_bytes());
                } else {
                    let m32 =
                        u32::try_from(m).map_err(|_| bad("a chunk offset no longer fits stco"))?;
                    nb.extend_from_slice(&m32.to_be_bytes());
                }
            }
            push_box(&mut out, if to_wide { b"co64" } else { b"stco" }, &nb);
        } else {
            push_box(&mut out, &kind, body);
        }
    }
    Ok(out)
}

fn push_box(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&((body.len() + 8) as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
}

/// A piece of the rearranged stream.
enum Seg {
    /// Bytes of the source, as they are.
    Src { from: u64, len: u64 },
    /// The patched `moov`.
    Mem(Vec<u8>),
}

impl Seg {
    fn len(&self) -> u64 {
        match self {
            Seg::Src { len, .. } => *len,
            Seg::Mem(v) => v.len() as u64,
        }
    }
}

/// The source read in its rearranged order. [`Read`] only — enough for the embedders and
/// [`io::copy`]; the length is known up front through [`FastStart::len`].
pub struct FastStart<R> {
    src: R,
    segs: Vec<Seg>,
    seg: usize,
    pos: u64,
    len: u64,
    changed: bool,
}

impl<R: Read + Seek> FastStart<R> {
    /// Plans the rearrangement. A file whose `moov` already precedes every `mdat` (or has no
    /// `mdat`) is passed through unchanged.
    pub fn open(src: R) -> io::Result<Self> {
        Self::open_with(src, false)
    }

    /// [`open`](Self::open) with `co64` forced (what a file over 4 GiB gets; for tests).
    pub fn open_with(mut src: R, force_wide: bool) -> io::Result<Self> {
        let (top, len) = scan(&mut src)?;
        let pass = |src, changed| FastStart {
            src,
            segs: vec![Seg::Src { from: 0, len }],
            seg: 0,
            pos: 0,
            len,
            changed,
        };
        let Some(mi) = top.iter().position(|b| &b.kind == b"moov") else {
            return Ok(pass(src, false));
        };
        let Some(di) = top.iter().position(|b| &b.kind == b"mdat") else {
            return Ok(pass(src, false));
        };
        if mi < di {
            return Ok(pass(src, false));
        }
        let moov = top[mi];
        let n = moov.size - moov.header;
        if n > MAX_MOOV {
            return Err(bad(format!("moov of {n} bytes is too large to rearrange")));
        }
        src.seek(SeekFrom::Start(moov.offset + moov.header))?;
        let mut raw = vec![0u8; n as usize];
        src.read_exact(&mut raw)?;

        // New order: top[..di], moov, top[di..] without moov. First the length of the new moov
        // (it may grow by widening), then the shifts, then the real patch.
        let zero = vec![0i64; top.len()];
        let mut stco_n = 0u64;
        let probe = rebuild(
            &raw,
            &Shift {
                top: &top,
                delta: &zero,
            },
            false,
            &mut stco_n,
        )?;
        let wide = force_wide || len + 4 * stco_n > u32::MAX as u64;
        let new_len = probe.len() as u64 + 8 + if wide { 4 * stco_n } else { 0 };
        let mut delta = vec![0i64; top.len()];
        let mut at = 0u64;
        let mut order = Vec::with_capacity(top.len());
        for (i, b) in top.iter().enumerate() {
            if i == di {
                at += new_len;
            }
            if i == mi {
                continue;
            }
            delta[i] = at as i64 - b.offset as i64;
            at += b.size;
            order.push(i);
        }
        let mut n2 = 0;
        let body = rebuild(
            &raw,
            &Shift {
                top: &top,
                delta: &delta,
            },
            wide,
            &mut n2,
        )?;
        let mut moov_new = Vec::with_capacity(body.len() + 8);
        push_box(&mut moov_new, b"moov", &body);
        debug_assert_eq!(moov_new.len() as u64, new_len);

        let mut segs: Vec<Seg> = Vec::new();
        let push_src = |segs: &mut Vec<Seg>, b: &Top| match segs.last_mut() {
            Some(Seg::Src { from, len }) if *from + *len == b.offset => *len += b.size,
            _ => segs.push(Seg::Src {
                from: b.offset,
                len: b.size,
            }),
        };
        for &i in &order {
            if i == di {
                segs.push(Seg::Mem(std::mem::take(&mut moov_new)));
            }
            push_src(&mut segs, &top[i]);
        }
        let total = segs.iter().map(Seg::len).sum();
        Ok(FastStart {
            src,
            segs,
            seg: 0,
            pos: 0,
            len: total,
            changed: true,
        })
    }

    /// Length of the rearranged stream (the source's, plus 4 bytes per widened chunk offset).
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// `false` when the source is streamed as it is.
    pub fn changed(&self) -> bool {
        self.changed
    }
}

impl<R: Read + Seek> Read for FastStart<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let Some(seg) = self.segs.get(self.seg) else {
                return Ok(0);
            };
            let left = seg.len() - self.pos;
            if left == 0 {
                self.seg += 1;
                self.pos = 0;
                continue;
            }
            let want = (buf.len() as u64).min(left) as usize;
            let n = match seg {
                Seg::Mem(v) => {
                    let p = self.pos as usize;
                    buf[..want].copy_from_slice(&v[p..p + want]);
                    want
                }
                Seg::Src { from, .. } => {
                    self.src.seek(SeekFrom::Start(from + self.pos))?;
                    let n = self.src.read(&mut buf[..want])?;
                    if n == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "the source ended inside a box",
                        ));
                    }
                    n
                }
            };
            self.pos += n as u64;
            return Ok(n);
        }
    }
}

/// Rearranges an MP4 file in place (through a sibling `.fsx` file and a rename). Returns
/// whether anything moved.
pub fn rewrite_file(path: &Path) -> io::Result<bool> {
    let mut fs = FastStart::open(io::BufReader::new(std::fs::File::open(path)?))?;
    if !fs.changed() {
        return Ok(false);
    }
    let tmp = path.with_extension("fsx");
    let r = (|| {
        let mut out = io::BufWriter::new(std::fs::File::create(&tmp)?);
        io::copy(&mut fs, &mut out)?;
        out.flush()
    })();
    drop(fs);
    if let Err(e) = r {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path)?;
    Ok(true)
}

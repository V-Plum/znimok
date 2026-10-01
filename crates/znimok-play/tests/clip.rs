//! The player on a clip recorded elsewhere (macOS has no recorder yet — ZK-88): the synthetic
//! barcode clip that `tests/play.rs` keeps with ZNIMOK_PLAY_KEEP=<file>. Run with
//! ZNIMOK_PLAY_CLIP=<file> (30 fps, 640 × 360, frame n shows the number n); without it the test
//! passes quietly.

use std::sync::mpsc;
use std::time::Duration;

use znimok_play::{Converter, Event, Gpu, Player, Source, Thumbs};

const W: u32 = 640;
const H: u32 = 360;

fn device() -> Option<Gpu> {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = if cfg!(target_os = "macos") {
        wgpu::Backends::METAL
    } else {
        wgpu::Backends::DX12
    };
    let instance = wgpu::Instance::new(desc);
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
    let features = adapter.features() & wgpu::Features::TEXTURE_FORMAT_NV12;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: features,
        ..Default::default()
    }))
    .ok()?;
    eprintln!("wgpu: {}", adapter.get_info().name);
    Some(Gpu { device, queue })
}

/// The barcode's number in an RGBA frame (blocks of width / 40, 16 bits then their inverse).
fn read_rgba(px: &[u8], w: u32, h: u32) -> Option<u32> {
    let blk = ((w / 40) & !1).max(2);
    let mut n = 0u32;
    for i in 0..32u32 {
        let (x, y) = (blk + i * blk + blk / 2, blk + blk / 2);
        if x >= w || y >= h {
            return None;
        }
        let o = ((y * w + x) * 4) as usize;
        let on = (u32::from(px[o]) + u32::from(px[o + 1]) + u32::from(px[o + 2])) / 3 > 125;
        if i < 16 {
            n |= u32::from(on) << i;
        } else if on == ((n >> (i - 16)) & 1 == 1) {
            return None;
        }
    }
    Some(n)
}

struct Rig {
    conv: Converter,
    player: Player,
    rx: mpsc::Receiver<Event>,
}

impl Rig {
    fn open(gpu: &Gpu, source: Source, frames: i64) -> Rig {
        let (tx, rx) = mpsc::channel();
        let player = Player::open(gpu.clone(), source, frames, move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
            Event::Opened(i) => {
                eprintln!(
                    "player: {}×{} at {} fps, path {}",
                    i.width, i.height, i.fps, i.path
                );
                assert_eq!((i.width, i.height), (W, H));
            }
            Event::Failed(e) => panic!("player failed: {e}"),
            _ => panic!("no Opened first"),
        }
        Rig {
            conv: Converter::new(gpu).unwrap(),
            player,
            rx,
        }
    }

    fn shown(&self) -> Option<(i64, Option<u32>, bool)> {
        loop {
            match self.rx.recv_timeout(Duration::from_secs(10)).ok()? {
                Event::Frame => {
                    let s = self.player.take()?;
                    let px = self.conv.read(&s.texture).unwrap();
                    return Some((s.frame, read_rgba(&px, W, H), s.playing));
                }
                Event::Failed(e) => panic!("{e}"),
                _ => continue,
            }
        }
    }

    fn seek(&self, f: i64) -> (i64, Option<u32>) {
        self.player.seek(f);
        let (got, bar, _) = self.shown().expect("a frame after a seek");
        (got, bar)
    }
}

#[test]
fn plays_the_kept_clip() {
    let Some(clip) = std::env::var_os("ZNIMOK_PLAY_CLIP") else {
        eprintln!("skipped: ZNIMOK_PLAY_CLIP is not set");
        return;
    };
    let Some(gpu) = device() else {
        eprintln!("skipped: no wgpu device");
        return;
    };
    let clip = std::path::PathBuf::from(clip);
    let frames = 90;
    let r = Rig::open(&gpu, Source::File(clip.clone()), frames);
    for f in [0, 45, 89, 10, 11, 60, 7] {
        assert_eq!(r.seek(f), (f, Some(f as u32)), "seek to {f}");
    }
    r.player.play(60, 0.5, false, false);
    let mut last = -1;
    while let Some((f, bar, playing)) = r.shown() {
        assert!(f > last || !playing, "forward: {f} after {last}");
        assert_eq!(bar, Some(f as u32), "forward frame {f}");
        last = f;
        if !playing {
            break;
        }
    }
    assert_eq!(last, frames - 1, "played to the end");
    r.player.play(70, 0.25, false, true);
    let mut last = i64::MAX;
    let mut seen = 0;
    while let Some((f, bar, playing)) = r.shown() {
        if !playing {
            break;
        }
        assert!(f < last, "reverse: {f} after {last}");
        assert_eq!(bar, Some(f as u32), "reverse frame {f}");
        last = f;
        seen += 1;
        if f < 60 {
            r.player.pause();
            break;
        }
    }
    assert!(seen >= 4, "only {seen} reverse frames");
    drop(r);

    // The MP4 as byte ranges of a bigger file (copied out into the cache where AVFoundation
    // needs a whole file).
    let dir = std::env::temp_dir().join(format!("znimok-play-clip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let data = std::fs::read(&clip).unwrap();
    let cut = data.len() / 2;
    let mut doc = b"HEAD".to_vec();
    let a0 = doc.len() as u64;
    doc.extend_from_slice(&data[..cut]);
    doc.extend_from_slice(b"--");
    let b0 = doc.len() as u64;
    doc.extend_from_slice(&data[cut..]);
    let file = dir.join("doc.bin");
    std::fs::write(&file, &doc).unwrap();
    let src = Source::InFile {
        path: file,
        ranges: vec![a0..a0 + cut as u64, b0..b0 + (data.len() - cut) as u64],
        cache: dir.join("cache"),
    };
    let r = Rig::open(&gpu, src.clone(), frames);
    for f in [33, 3, 88] {
        assert_eq!(r.seek(f), (f, Some(f as u32)), "in a document, frame {f}");
    }
    drop(r);
    let (tx, rx) = mpsc::channel();
    let _t = Thumbs::start(gpu, src, vec![0, 30, 60, 89], 32, None, move |f, r| {
        let _ = tx.send((f, r.width, r.height));
    });
    let mut got = Vec::new();
    while let Ok(t) = rx.recv_timeout(Duration::from_secs(10)) {
        got.push(t);
        if got.len() == 4 {
            break;
        }
    }
    assert_eq!(got.len(), 4, "{got:?}");
    // Read in place on both systems (ZK-200): nothing was copied into the cache.
    let copied = std::fs::read_dir(dir.join("cache")).map_or(0, |d| d.count());
    assert_eq!(copied, 0, "the MP4 was copied out of the document");
    let _ = std::fs::remove_dir_all(&dir);
}

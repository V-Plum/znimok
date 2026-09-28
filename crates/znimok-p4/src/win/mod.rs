//! Windows commands of the P4 prototype.

mod interop;
mod mf;

use crate::gpu::{Gpu, Planes};
use crate::pattern;
use mf::{Mode, Reader};
use serde_json::json;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|s| s == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn num<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> Result<T, String> {
    match flag(args, name) {
        Some(v) => v.parse().map_err(|_| format!("{name}: не число «{v}»")),
        None => Ok(default),
    }
}

pub fn run(cmd: &str, args: &[String]) -> Result<(), String> {
    mf::startup()?;
    match cmd {
        "info" => info(),
        "gen" => gen_clip(args),
        "bench" => bench(args),
        "seek" => seek(args),
        "quality" => quality(args),
        _ => Err(format!("невідома команда «{cmd}»")),
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn info() -> Result<(), String> {
    let gpu = Gpu::new()?;
    let bridge = interop::Bridge::new(&gpu);
    println!(
        "{}",
        json!({
            "wgpu_adapter": gpu.adapter.name, "backend": format!("{:?}", gpu.adapter.backend),
            "nv12_textures": gpu.nv12,
            "d3d11_on_same_adapter": bridge.as_ref().map(|b| json!({"adapter": b.adapter, "luid": b.luid})).unwrap_or_else(|e| json!({"error": e})),
        })
    );
    Ok(())
}

fn gen_clip(args: &[String]) -> Result<(), String> {
    let out = flag(args, "-o").unwrap_or("p4-4k60.mp4");
    let (w, h) = flag(args, "--size")
        .unwrap_or("3840x2160")
        .split_once('x')
        .and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
        .ok_or("--size ШxВ")?;
    let fps: u32 = num(args, "--fps", 60)?;
    let seconds: u32 = num(args, "--seconds", 20)?;
    let p = mf::GenParams {
        width: w,
        height: h,
        fps,
        frames: fps * seconds,
        gop: num(args, "--gop", fps)?,
        bits_per_pixel: num(args, "--bpp", 0.10)?,
    };
    // Absolute path: MF resolves URLs itself.
    let abs = std::path::absolute(out).map_err(|e| e.to_string())?;
    println!("{}", mf::generate(&abs.to_string_lossy(), &p)?);
    Ok(())
}

/// CPU time of this process (user + kernel), all threads including the decoder's.
fn cpu_time() -> Duration {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let (mut c, mut x, mut k, mut u) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe {
        let _ = GetProcessTimes(GetCurrentProcess(), &mut c, &mut x, &mut k, &mut u);
    }
    let t = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
    Duration::from_nanos((t(k) + t(u)) * 100)
}

/// Frames the fallback path keeps in flight before mapping (so `Map` never waits for the copy);
/// `--lag` overrides.
const STAGING_LAG: usize = 2;

/// One playback pipeline: reader + whichever way frames reach wgpu.
struct Player {
    gpu: Gpu,
    reader: Reader,
    out: wgpu::Texture,
    mode: Mode,
    bridge: Option<interop::Bridge>,
    planes: Option<Planes>,
    bind: Option<wgpu::BindGroup>,
    y: Vec<u8>,
    uv: Vec<u8>,
    /// Frame indices copied into the staging ring and not yet uploaded (fallback path).
    pending: VecDeque<u32>,
    frames_on_gpu: Option<bool>,
    lag: usize,
    /// Fallback path: time spent in Map + plane copy, and in the wgpu upload + submit.
    ms_map: f64,
    ms_upload: f64,
}

impl Player {
    fn open(
        path: &str,
        mode: Mode,
        pool: usize,
        lag: usize,
        low_latency: bool,
    ) -> Result<Self, String> {
        let gpu = Gpu::new()?;
        let abs = std::path::absolute(path).map_err(|e| e.to_string())?;
        let path = abs.to_string_lossy();
        let mut bridge = match mode {
            Mode::Software => None,
            _ => Some(interop::Bridge::new(&gpu)?),
        };
        let reader = Reader::open(&path, bridge.as_ref().map(|b| &b.manager), low_latency)?;
        let (w, h) = (reader.width, reader.height);
        let out = gpu.target(w, h);
        let (mut planes, mut bind) = (None, None);
        if mode == Mode::Zero {
            bridge.as_mut().unwrap().alloc(&gpu, &out, w, h, pool)?;
        } else {
            if mode == Mode::Cpu {
                bridge.as_mut().unwrap().alloc_staging(w, h, lag + 1)?;
            }
            let p = gpu.planes(w, h);
            bind = Some(gpu.bind(
                &p.y.create_view(&Default::default()),
                &p.uv.create_view(&Default::default()),
                &out,
            ));
            planes = Some(p);
        }
        Ok(Self {
            gpu,
            reader,
            out,
            mode,
            bridge,
            planes,
            bind,
            y: Vec::new(),
            uv: Vec::new(),
            pending: VecDeque::new(),
            frames_on_gpu: None,
            lag,
            ms_map: 0.0,
            ms_upload: 0.0,
        })
    }

    fn upload_current(&self) {
        let (w, h) = (self.reader.width, self.reader.height);
        self.gpu
            .upload(self.planes.as_ref().unwrap(), &self.y, w, &self.uv, w);
        self.gpu
            .queue
            .submit([self.gpu.convert(self.bind.as_ref().unwrap(), w, h)]);
    }

    /// Hand one decoded frame to the GPU and queue its conversion. Returns the index of the frame
    /// that is now in the output (the fallback path shows frames `lag` late).
    fn show(&mut self, s: &mf::Decoded) -> Result<Option<u32>, String> {
        let (w, h) = (self.reader.width, self.reader.height);
        let idx = self.reader.index_of(s.timestamp);
        self.frames_on_gpu
            .get_or_insert_with(|| interop::on_gpu(&s.sample));
        match self.mode {
            Mode::Zero => {
                self.bridge
                    .as_mut()
                    .unwrap()
                    .present(&self.gpu, &s.sample)?;
                Ok(Some(idx))
            }
            Mode::Cpu => {
                self.bridge.as_mut().unwrap().stage(&s.sample)?;
                self.pending.push_back(idx);
                if self.pending.len() > self.lag {
                    self.flush()
                } else {
                    Ok(None)
                }
            }
            Mode::MfLock | Mode::Software => {
                mf::lock_planes(
                    &s.sample,
                    w,
                    h,
                    self.reader.alloc_height,
                    &mut self.y,
                    &mut self.uv,
                )?;
                self.upload_current();
                Ok(Some(idx))
            }
        }
    }

    /// Fallback path: upload the oldest staged frame, if any.
    fn flush(&mut self) -> Result<Option<u32>, String> {
        if self.mode != Mode::Cpu {
            return Ok(None);
        }
        let t = Instant::now();
        let (gpu, planes) = (&self.gpu, self.planes.as_ref().unwrap());
        let put = |y: &[u8], uv: &[u8], pitch: u32| gpu.upload(planes, y, pitch, uv, pitch);
        if !self.bridge.as_mut().unwrap().unstage(put)? {
            return Ok(None);
        }
        let t1 = Instant::now();
        let (w, h) = (self.reader.width, self.reader.height);
        self.gpu
            .queue
            .submit([self.gpu.convert(self.bind.as_ref().unwrap(), w, h)]);
        self.ms_map += (t1 - t).as_secs_f64() * 1000.0;
        self.ms_upload += t1.elapsed().as_secs_f64() * 1000.0;
        Ok(self.pending.pop_front())
    }

    /// After a seek or at the end: push everything still staged through, return the last shown index.
    fn drain(&mut self, shown: Option<u32>) -> Result<Option<u32>, String> {
        let mut last = shown;
        while let Some(i) = self.flush()? {
            last = Some(i);
        }
        Ok(last)
    }

    /// Read the barcode of the last converted frame.
    fn check(&self) -> pattern::Reading {
        let band = self
            .gpu
            .read_band(&self.out, pattern::band_rows(self.reader.width));
        pattern::read(&band, self.reader.width)
    }

    fn describe(&self) -> serde_json::Value {
        json!({
            "mode": self.mode.name(), "size": [self.reader.width, self.reader.height],
            "fps": round1(self.reader.fps()), "frames_in_file": self.reader.frame_count(),
            "decoded_frames_in": match self.frames_on_gpu { Some(true) => "gpu (DXVA)", Some(false) => "cpu", None => "?" },
            "low_latency": self.reader.low_latency,
            "wgpu_adapter": self.gpu.adapter.name,
            "d3d11_adapter": self.bridge.as_ref().map(|b| format!("{} (LUID {})", b.adapter, b.luid)),
        })
    }
}

fn open_player(args: &[String], usage: &str) -> Result<Player, String> {
    let path = args
        .first()
        .filter(|a| !a.starts_with('-'))
        .ok_or(usage.to_string())?;
    let mode =
        Mode::parse(flag(args, "--mode").unwrap_or("zero")).ok_or("--mode zero|cpu|mflock|sw")?;
    Player::open(
        path,
        mode,
        num(args, "--pool", 4)?,
        num(args, "--lag", STAGING_LAG)?,
        args.iter().any(|a| a == "--lowlat"),
    )
}

fn percentile(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}

/// Play the whole file: as fast as possible, or paced at its own frame rate (`--paced`), which is
/// the CPU-load measurement. `--verify` reads every frame's barcode back (slows it down).
fn bench(args: &[String]) -> Result<(), String> {
    let mut p = open_player(
        args,
        "bench <файл> [--mode zero|cpu|mflock|sw] [--paced] [--verify] [--frames N] [--lowlat]",
    )?;
    let paced = args.iter().any(|a| a == "--paced");
    let verify = args.iter().any(|a| a == "--verify");
    let limit: u32 = num(args, "--frames", u32::MAX)?;
    let period = Duration::from_secs_f64(1.0 / p.reader.fps());
    let (mut frames, mut wrong, mut damaged, mut patch_worst) = (0u32, 0u32, 0u32, 0u8);
    let mut per_frame = Vec::new();
    let mut late = 0u32;
    let (t0, c0) = (Instant::now(), cpu_time());
    while frames < limit {
        let tf = Instant::now();
        let Some(s) = p.reader.next()? else { break };
        let shown = p.show(&s)?;
        let _ = p.gpu.device.poll(wgpu::PollType::Poll);
        per_frame.push(tf.elapsed().as_secs_f64() * 1000.0);
        if let (true, Some(want)) = (verify, shown) {
            let r = p.check();
            match r.index {
                Some(i) if i == want => {}
                Some(_) => wrong += 1,
                None => damaged += 1,
            }
            patch_worst = patch_worst.max(r.patch_max_diff);
        }
        frames += 1;
        if paced {
            let due = t0 + period * frames;
            let now = Instant::now();
            if now < due {
                std::thread::sleep(due - now);
            } else if now - due > period {
                late += 1;
            }
        }
    }
    p.drain(None)?;
    p.gpu.wait();
    let wall = t0.elapsed().as_secs_f64();
    let cpu = (cpu_time() - c0).as_secs_f64();
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get()) as f64;
    let mut out = p.describe();
    let o = out.as_object_mut().unwrap();
    o.insert("paced".into(), json!(paced));
    o.insert("frames".into(), json!(frames));
    o.insert("seconds".into(), json!(round1(wall)));
    o.insert(
        "fps_achieved".into(),
        json!(round1(f64::from(frames) / wall)),
    );
    o.insert(
        "frame_ms_p50".into(),
        json!(round1(percentile(&mut per_frame.clone(), 0.5))),
    );
    o.insert(
        "frame_ms_p95".into(),
        json!(round1(percentile(&mut per_frame, 0.95))),
    );
    o.insert(
        "cpu_percent_of_machine".into(),
        json!(round1(cpu / wall / cores * 100.0)),
    );
    o.insert(
        "cpu_percent_of_one_core".into(),
        json!(round1(cpu / wall * 100.0)),
    );
    o.insert("logical_cores".into(), json!(cores));
    if paced {
        o.insert("frames_late_over_one_period".into(), json!(late));
    }
    if p.mode == Mode::Cpu {
        let n = f64::from(frames.max(1));
        o.insert("lag".into(), json!(p.lag));
        o.insert(
            "map_and_upload_ms_per_frame".into(),
            json!(round1(p.ms_map / n)),
        );
        o.insert("submit_ms_per_frame".into(), json!(round1(p.ms_upload / n)));
    }
    if verify {
        o.insert(
            "verify".into(),
            json!({"wrong_index": wrong, "damaged_barcode": damaged, "patch_max_diff": patch_worst}),
        );
    }
    println!("{out}");
    Ok(())
}

/// Seek to random frames and to the worst case (the frame just before each key frame), time each
/// seek until the frame is converted on the GPU, and check the barcode says the right frame.
fn seek(args: &[String]) -> Result<(), String> {
    let mut p = open_player(
        args,
        "seek <файл> [--mode zero|cpu|mflock|sw] [--count N] [--gop N] [--seed N] [--lowlat] [--rows]",
    )?;
    let total = p.reader.frame_count().max(1);
    let gop: u32 = num(args, "--gop", p.reader.fps().round() as u32)?;
    let count: u32 = num(args, "--count", 40)?;
    let mut rng: u64 = num(args, "--seed", 7)?;
    let mut targets = Vec::new();
    for _ in 0..count {
        rng = rng
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        targets.push(((rng >> 33) % u64::from(total)) as u32);
    }
    // Worst case: the last frame of a GOP needs the whole GOP decoded.
    targets.extend((1..=8).map(|k| k * gop - 1).filter(|&f| f < total));
    // Warm-up (the first decode opens the decoder).
    let (f, _) = p.reader.seek(0)?;
    let shown = p.show(&f)?;
    p.drain(shown)?;
    p.gpu.wait();
    let mut rows = Vec::new();
    let (mut ms_all, mut ms_worst) = (Vec::new(), Vec::new());
    let (mut wrong, mut patch_worst) = (0u32, 0u8);
    for (n, &t) in targets.iter().enumerate() {
        let t0 = Instant::now();
        let (f, decoded) = p.reader.seek(t)?;
        let shown = p.show(&f)?;
        p.drain(shown)?;
        p.gpu.wait();
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        let r = p.check();
        if r.index != Some(t) {
            wrong += 1;
        }
        patch_worst = patch_worst.max(r.patch_max_diff);
        let gop_end = n >= count as usize;
        if gop_end {
            ms_worst.push(ms);
        }
        ms_all.push(ms);
        rows.push(json!({"frame": t, "got": r.index, "decoded": decoded, "ms": round1(ms), "gop_end": gop_end}));
    }
    let mut out = p.describe();
    let o = out.as_object_mut().unwrap();
    o.insert("gop".into(), json!(gop));
    o.insert("seeks".into(), json!(targets.len()));
    o.insert("wrong_frame".into(), json!(wrong));
    o.insert("patch_max_diff".into(), json!(patch_worst));
    o.insert(
        "ms_p50".into(),
        json!(round1(percentile(&mut ms_all.clone(), 0.5))),
    );
    o.insert(
        "ms_p95".into(),
        json!(round1(percentile(&mut ms_all.clone(), 0.95))),
    );
    o.insert("ms_max".into(), json!(round1(percentile(&mut ms_all, 1.0))));
    o.insert(
        "ms_max_gop_end".into(),
        json!(round1(percentile(&mut ms_worst, 1.0))),
    );
    if args.iter().any(|a| a == "--rows") {
        o.insert("rows".into(), json!(rows));
    }
    println!("{out}");
    Ok(())
}

/// Codec loss against the generator's own frames: PSNR of every `--every`-th decoded frame (RGB,
/// through the same BT.709 conversion) — the price of a shorter GOP at a fixed bitrate.
fn quality(args: &[String]) -> Result<(), String> {
    let mut p = open_player(args, "quality <файл> [--every N]")?;
    let every: u32 = num(args, "--every", 20)?;
    let (w, h) = (p.reader.width, p.reader.height);
    let mut reference = pattern::Nv12::new(w, h);
    let mut psnr = Vec::new();
    while let Some(s) = p.reader.next()? {
        let Some(idx) = p.show(&s)? else { continue };
        if idx % every != 0 {
            continue;
        }
        let got = p.gpu.read_band(&p.out, h);
        reference.draw(idx);
        let want = pattern::nv12_to_rgba(&reference);
        let (mut se, mut n) = (0f64, 0f64);
        for (a, b) in got.as_chunks::<4>().0.iter().zip(want.as_chunks::<4>().0) {
            for k in 0..3 {
                let d = f64::from(a[k]) - f64::from(b[k]);
                se += d * d;
                n += 1.0;
            }
        }
        let mse = (se / n).max(1e-9);
        psnr.push(10.0 * (255.0f64 * 255.0 / mse).log10());
    }
    let mean = psnr.iter().sum::<f64>() / psnr.len().max(1) as f64;
    let min = psnr.iter().copied().fold(f64::INFINITY, f64::min);
    let mut out = p.describe();
    let o = out.as_object_mut().unwrap();
    o.insert("frames_measured".into(), json!(psnr.len()));
    o.insert("psnr_mean_db".into(), json!(round1(mean)));
    o.insert("psnr_min_db".into(), json!(round1(min)));
    println!("{out}");
    Ok(())
}

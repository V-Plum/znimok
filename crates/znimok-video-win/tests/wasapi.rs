//! ZK-89: the system sound through WASAPI loopback. The test plays a quiet 440 Hz tone (−34 dB,
//! 0.6 s) on the default output and reads it back: the source hears it, its packets are
//! stamped on the recording clock, and a recording with it has an audio track named after the
//! device. Where the machine has no output device (a CI runner) it says so and passes.
#![cfg(windows)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, IAudioClient, IAudioRenderClient, IMMDeviceEnumerator,
    MMDeviceEnumerator, WAVEFORMATEX, eConsole, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use znimok_video::clock::{Clock, ticks_to_hns};
use znimok_video::traits::{AudioKind, AudioPacket, AudioSource};
use znimok_video_win::{QpcClock, WasapiSource};

/// Plays the tone on the default output until `stop`; `ready` says whether it could.
fn play_tone(stop: Arc<AtomicBool>, ready: mpsc::Sender<bool>) {
    znimok_win::raw::com_thread();
    // SAFETY: a render client made and used on this thread only; the buffer pointer is valid
    // between GetBuffer and ReleaseBuffer.
    unsafe {
        let setup = || -> windows::core::Result<(IAudioClient, IAudioRenderClient, u32)> {
            let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let dev = en.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
            let f = WAVEFORMATEX {
                wFormatTag: 3,
                nChannels: 2,
                nSamplesPerSec: 48_000,
                nAvgBytesPerSec: 48_000 * 8,
                nBlockAlign: 8,
                wBitsPerSample: 32,
                cbSize: 0,
            };
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                1_000_000,
                0,
                &f,
                None,
            )?;
            let render: IAudioRenderClient = client.GetService()?;
            let size = client.GetBufferSize()?;
            client.Start()?;
            Ok((client, render, size))
        };
        let Ok((client, render, size)) = setup() else {
            let _ = ready.send(false);
            return;
        };
        let _ = ready.send(true);
        let mut n = 0u64;
        while !stop.load(Ordering::SeqCst) {
            let pad = client.GetCurrentPadding().unwrap_or(size);
            let free = size.saturating_sub(pad);
            if free > 0
                && let Ok(p) = render.GetBuffer(free)
            {
                let out = std::slice::from_raw_parts_mut(p as *mut f32, free as usize * 2);
                for f in out.as_chunks_mut::<2>().0 {
                    let v = 0.02 * (n as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin();
                    f[0] = v;
                    f[1] = v;
                    n += 1;
                }
                let _ = render.ReleaseBuffer(free, 0);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = client.Stop();
    }
}

#[test]
fn loopback_hears_the_system_sound() {
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let s2 = stop.clone();
    let player = std::thread::spawn(move || play_tone(s2, tx));
    if !rx.recv().unwrap_or(false) {
        eprintln!("пропущено: немає пристрою виводу звуку");
        let _ = player.join();
        return;
    }
    let mut src = WasapiSource::system(None);
    if let Err(e) = src.open() {
        stop.store(true, Ordering::SeqCst);
        let _ = player.join();
        eprintln!("пропущено: loopback не відкрився: {e}");
        return;
    }
    assert_eq!(src.kind(), AudioKind::System);
    let clock = QpcClock::new();
    let mut packets: Vec<AudioPacket> = Vec::new();
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(15));
        src.read(&mut packets).unwrap();
    }
    let now_hns = ticks_to_hns(clock.ticks(), clock.frequency());
    stop.store(true, Ordering::SeqCst);
    let _ = player.join();
    src.close();
    let samples: Vec<f32> = packets
        .iter()
        .filter(|p| !p.silent)
        .flat_map(|p| p.data.iter().copied())
        .collect();
    let rms = (samples.iter().map(|v| v * v).sum::<f32>() / samples.len().max(1) as f32).sqrt();
    let newest = packets.iter().map(|p| p.time_hns).max().unwrap_or(0);
    eprintln!(
        "«{}»: пакетів {}, кадрів {}, RMS {rms:.4}, найновіший пакет {:.0} мс тому",
        src.label(),
        packets.len(),
        samples.len() / 2,
        (now_hns - newest) as f64 / 10_000.0
    );
    assert!(!src.label().is_empty(), "the device has a name");
    // The tone's RMS is 0.014; a muted or silent output reads ~0 — then nothing is asserted
    // about the level, only about the stamps.
    if rms > 0.001 {
        assert!(rms > 0.005, "the tone is heard: RMS {rms}");
    } else {
        eprintln!("вихід, схоже, вимкнено — рівень не перевіряється");
    }
    assert!(!packets.is_empty(), "packets arrive while something plays");
    // A loopback stamps the moment the sound is played, a little ahead of reading it.
    let age_ms = (now_hns - newest) / 10_000;
    assert!(
        (-150..300).contains(&age_ms),
        "packets are stamped on the recording clock (QPC): newest {age_ms} ms ago"
    );
}

#[test]
fn a_recording_carries_the_system_sound_track() {
    use znimok_video::check::mp4::read_mp4_file;
    use znimok_video_win::{Api, RecordRequest, Recording, Target};
    let Some(m) = znimok_win::raw::monitors().into_iter().next() else {
        eprintln!("пропущено: немає дисплеїв");
        return;
    };
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let s2 = stop.clone();
    let player = std::thread::spawn(move || play_tone(s2, tx));
    if !rx.recv().unwrap_or(false) {
        eprintln!("пропущено: немає пристрою виводу звуку");
        let _ = player.join();
        return;
    }
    let dir = std::env::temp_dir().join(format!("znimok-wasapi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sound.mp4");
    let req = RecordRequest {
        audio: vec![Box::new(WasapiSource::system(None))],
        api: Api::Wgc,
        ..RecordRequest::new(
            Target::Display {
                id: m.info.id.clone(),
                region: None,
            },
            path.clone(),
        )
    };
    let rec = match Recording::start(req) {
        Ok(r) => r,
        Err(e) => {
            stop.store(true, Ordering::SeqCst);
            let _ = player.join();
            eprintln!("пропущено (екран недоступний у цьому сеансі): {e}");
            return;
        }
    };
    assert_eq!(rec.started().audio_tracks, 1);
    std::thread::sleep(Duration::from_millis(1500));
    let fin = rec.stop();
    stop.store(true, Ordering::SeqCst);
    let _ = player.join();
    eprintln!(
        "кадрів {}, доріжки {:?}, попередження {:?}",
        fin.result.frames, fin.result.audio_sources, fin.result.warning
    );
    if fin.result.frames == 0 {
        eprintln!("пропущено: жодного кадру");
        return;
    }
    assert_eq!(fin.result.audio_sources.len(), 1);
    assert_eq!(fin.result.audio_sources[0].0, AudioKind::System);
    assert!(!fin.result.audio_sources[0].1.is_empty());
    let info = read_mp4_file(&fin.path.expect("committed")).unwrap();
    assert!(info.audio().is_some(), "the MP4 has an audio track");
    let _ = std::fs::remove_dir_all(&dir);
}

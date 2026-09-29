//! Sparkle through Znimok's driver, for real (ZK-143): the framework is loaded, an updater runs
//! for a fake host bundle whose `SUFeedURL` points at a local HTTP server, and the driver's
//! events are what the app would see. No harness: Sparkle calls the driver on the main
//! thread, so this test IS the main thread and runs the run loop itself.
//!
//! `ZNIMOK_SPARKLE_FRAMEWORK=/path/to/Sparkle.framework` — without it the test only checks
//! that the driver class answers every selector of the protocol.
#[cfg(target_os = "macos")]
mod live {

    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    use objc2_foundation::{MainThreadMarker, NSDate, NSRunLoop};
    use znimok_mac::sparkle::{Choice, Event, Sparkle, driver_responds_to_all};

    const PUB: &str = include_str!("../../../keys/znimok-sparkle-ed25519.pub.pem");

    /// Serve one appcast at `/appcast.xml` for as long as the test runs.
    fn serve(appcast: String) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let mut s = s;
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let body = appcast.as_bytes();
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/rss+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(body);
            }
        });
        port
    }

    fn appcast(version: &str, port: u16) -> String {
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
    <rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
      <channel><title>Test</title><item>
        <title>Test {version}</title>
        <sparkle:version>{version}</sparkle:version>
        <sparkle:shortVersionString>{version}</sparkle:shortVersionString>
        <sparkle:minimumSystemVersion>12.0</sparkle:minimumSystemVersion>
        <sparkle:releaseNotesLink>http://127.0.0.1:{port}/notes.html</sparkle:releaseNotesLink>
        <enclosure url="http://127.0.0.1:{port}/Test-{version}.zip" length="1234" type="application/octet-stream" sparkle:edSignature="AAAA"/>
      </item></channel>
    </rss>
    "#
        )
    }

    /// A host bundle Sparkle accepts: an Info.plist with the key and a feed, and an executable.
    fn host_bundle(dir: &Path, version: &str, port: u16) -> PathBuf {
        let key: String = PUB
            .lines()
            .filter(|l| !l.starts_with("-----"))
            .collect::<String>();
        // The PEM holds the SubjectPublicKeyInfo (44 bytes); SUPublicEDKey wants the raw 32.
        let der = base64_decode(&key);
        let raw = base64_encode(&der[der.len() - 32..]);
        let app = dir.join("ZnimokSparkleTest.app");
        let contents = app.join("Contents");
        std::fs::create_dir_all(contents.join("MacOS")).unwrap();
        std::fs::write(
            contents.join("Info.plist"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
    <plist version="1.0"><dict>
    <key>CFBundleIdentifier</key><string>ua.plum.znimok.sparkletest</string>
    <key>CFBundleName</key><string>ZnimokSparkleTest</string>
    <key>CFBundleExecutable</key><string>test</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>{version}</string>
    <key>CFBundleVersion</key><string>{version}</string>
    <key>SUFeedURL</key><string>http://127.0.0.1:{port}/appcast.xml</string>
    <key>SUPublicEDKey</key><string>{raw}</string>
    <key>SUEnableAutomaticChecks</key><false/>
    </dict></plist>
    "#
            ),
        )
        .unwrap();
        std::fs::copy(
            std::env::current_exe().unwrap(),
            contents.join("MacOS").join("test"),
        )
        .unwrap();
        app
    }

    fn base64_decode(s: &str) -> Vec<u8> {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        let mut acc = 0u32;
        let mut bits = 0;
        for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
            let v = T.iter().position(|t| *t == c).unwrap() as u32;
            acc = (acc << 6) | v;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
                acc &= (1 << bits) - 1;
            }
        }
        out
    }

    fn base64_encode(b: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for c in b.chunks(3) {
            let n = (u32::from(c[0]) << 16)
                | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
                | u32::from(*c.get(2).unwrap_or(&0));
            for i in 0..4 {
                if i <= c.len() {
                    s.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
                } else {
                    s.push('=');
                }
            }
        }
        s
    }

    /// Run the main run loop until `f` says stop, or `secs` pass.
    fn run_loop_until(secs: f64, mut f: impl FnMut() -> bool) -> bool {
        let t0 = Instant::now();
        while t0.elapsed().as_secs_f64() < secs {
            NSRunLoop::mainRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.05));
            if f() {
                return true;
            }
        }
        false
    }

    pub fn main() {
        let mtm = MainThreadMarker::new().expect("the test binary's main thread");
        let missing = driver_responds_to_all(mtm);
        assert!(missing.is_empty(), "the driver lacks {missing:?}");
        println!("ok   driver answers every SPUUserDriver selector");

        let Some(fw) = std::env::var_os("ZNIMOK_SPARKLE_FRAMEWORK") else {
            println!("skip ZNIMOK_SPARKLE_FRAMEWORK is not set: the live part is not run");
            return;
        };
        let fw = PathBuf::from(fw);
        let dir = std::env::temp_dir().join(format!("znimok-sparkle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // 1. A newer version in the feed: a background check ends in «found», with the version.
        let port = serve(appcast("99.0.0", 0));
        let host = host_bundle(&dir, "1.0.0", port);
        let s = Sparkle::start(&fw, Some(&host), None).expect("start");
        assert!(s.can_check());
        s.check(false);
        let mut events = Vec::new();
        let found = run_loop_until(20.0, || {
            events.extend(s.poll());
            events
                .iter()
                .any(|e| matches!(e, Event::Found { .. } | Event::Error(_)))
        });
        println!("{events:?}");
        assert!(found, "no answer from Sparkle in 20 s");
        match events.iter().find(|e| matches!(e, Event::Found { .. })) {
            Some(Event::Found {
                version,
                user_initiated,
                stage,
                ..
            }) => {
                assert_eq!(version, "99.0.0");
                assert!(!user_initiated);
                assert_eq!(*stage, znimok_mac::sparkle::Stage::NotDownloaded);
            }
            other => panic!("expected Found, got {other:?}"),
        }
        assert!(s.awaits_reply());
        assert!(s.reply(Choice::Dismiss));
        assert!(!s.awaits_reply());
        run_loop_until(2.0, || false);
        println!("ok   a newer version is found and can be dismissed");
        drop(s);

        // 2. An older version in the feed: a check the person starts ends in «not found».
        let port = serve(appcast("0.0.1", 0));
        let host = host_bundle(&dir.join("older"), "1.0.0", port);
        let s = Sparkle::start(&fw, Some(&host), None).expect("start");
        s.check(true);
        let mut events = Vec::new();
        let done = run_loop_until(20.0, || {
            events.extend(s.poll());
            events
                .iter()
                .any(|e| matches!(e, Event::NotFound(_) | Event::Error(_)))
        });
        println!("{events:?}");
        assert!(done, "no answer from Sparkle in 20 s");
        assert!(
            events.contains(&Event::Checking),
            "the user-started check was announced"
        );
        assert!(
            events.iter().any(|e| matches!(e, Event::NotFound(_))),
            "{events:?}"
        );
        println!("ok   an older version is «not found» after a check the person started");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(target_os = "macos")]
fn main() {
    live::main();
}

#[cfg(not(target_os = "macos"))]
fn main() {}

//! macOS / Linux transport: a unix socket `znimok[-suffix].sock` in a `0700` folder, the socket
//! file itself `0600`; a stale socket from a crashed run is replaced only if nobody answers on it.

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use crate::{Config, Handler, Split, serve};

pub(crate) fn socket_path(cfg: &Config) -> PathBuf {
    cfg.dir().join(format!("znimok{}.sock", cfg.tag()))
}

pub(crate) struct Listener {
    path: PathBuf,
    stop: Arc<AtomicBool>,
}

impl Listener {
    pub fn start(
        cfg: Config,
        token: Arc<crate::Auth>,
        handler: Arc<dyn Handler>,
        stop: Arc<AtomicBool>,
        active: Arc<AtomicUsize>,
    ) -> std::io::Result<Self> {
        let path = socket_path(&cfg);
        if path.exists() {
            if UnixStream::connect(&path).is_ok() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    "another Znimok server is running",
                ));
            }
            std::fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let s = stop.clone();
        std::thread::Builder::new()
            .name("znimok-ipc".into())
            .spawn(move || {
                let cfg = Arc::new(cfg);
                for stream in listener.incoming() {
                    if s.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    if active.load(Ordering::SeqCst) >= cfg.max_clients {
                        continue; // dropped: too many clients
                    }
                    let (token, cfg, handler, active) =
                        (token.clone(), cfg.clone(), handler.clone(), active.clone());
                    std::thread::spawn(move || {
                        active.fetch_add(1, Ordering::SeqCst);
                        if let Ok(read) = stream.try_clone() {
                            let t = stream.try_clone();
                            let timeout = Box::new(move |d: Option<Duration>| {
                                if let Ok(t) = &t {
                                    let _ = t.set_read_timeout(d);
                                }
                            });
                            serve(
                                Split {
                                    read: Box::new(read),
                                    write: Box::new(stream),
                                    timeout,
                                },
                                &token,
                                &cfg,
                                &*handler,
                            );
                        }
                        active.fetch_sub(1, Ordering::SeqCst);
                    });
                }
            })?;
        Ok(Self { path, stop })
    }

    pub fn endpoint(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = UnixStream::connect(&self.path); // wake accept()
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(crate) fn connect(
    cfg: &Config,
) -> std::io::Result<(Box<dyn Read + Send>, Box<dyn Write + Send>)> {
    connect_endpoint(&socket_path(cfg).to_string_lossy())
}

/// Connects to a socket by its path (the probe of another user's socket uses it).
pub(crate) fn connect_endpoint(
    path: &str,
) -> std::io::Result<(Box<dyn Read + Send>, Box<dyn Write + Send>)> {
    let s = UnixStream::connect(path)?;
    let r = s.try_clone()?;
    Ok((Box::new(r), Box::new(s)))
}

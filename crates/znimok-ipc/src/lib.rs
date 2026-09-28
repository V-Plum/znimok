//! Local IPC of Znimok (ZK-31, PLAN §5.9–5.10): the transport for CLI → GUI, the MCP proxy and
//! tests. JSON-RPC 2.0, one message per line.
//!
//! - **Endpoint** — Windows: named pipe `\\.\pipe\znimok-<user SID>` whose DACL admits only the
//!   current user, remote clients rejected, first instance claimed (`FILE_FLAG_FIRST_PIPE_INSTANCE`,
//!   no squatting). macOS / Linux: `znimok.sock` in a `0700` folder, the socket itself `0600`.
//!   No TCP port anywhere.
//! - **Session token** — 32 random bytes, new on every server start, written to `ipc-token` next
//!   to the user's profile data (readable by that user only). The first request of a connection
//!   must be `hello {token, client}`; anything else closes it.
//! - **Limits** — request size (1 MiB), time to say hello (5 s), idle time (10 min), clients (8).
//!
//! ```no_run
//! use znimok_ipc::{Config, Server, Client, RpcError};
//! use serde_json::{json, Value};
//! let server = Server::start(Config::default(), |method: &str, params: Value| match method {
//!     "ping" => Ok(json!("pong")),
//!     _ => Err(RpcError::method_not_found(method)),
//! }).unwrap();
//! let mut c = Client::connect(&Config::default(), "cli").unwrap();
//! assert_eq!(c.call("ping", json!(null)).unwrap(), json!("pong"));
//! drop(server);
//! ```

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod win;

/// Version of the message set on top of JSON-RPC; bumped when `hello` or framing changes.
pub const PROTOCOL: u32 = 1;

#[derive(Clone, Debug)]
pub struct Config {
    /// Added to the endpoint and token names — tests and a second instance use it.
    pub suffix: Option<String>,
    /// Folder for the token file and the unix socket; `None` = the OS default ([`default_dir`]).
    pub dir: Option<PathBuf>,
    pub max_request: usize,
    pub hello_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_clients: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            suffix: None,
            dir: None,
            max_request: 1 << 20,
            hello_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(600),
            max_clients: 8,
        }
    }
}

/// `%LOCALAPPDATA%\Znimok`, `~/Library/Application Support/Znimok`, `$XDG_RUNTIME_DIR/znimok`.
pub fn default_dir() -> PathBuf {
    let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
    if cfg!(windows) {
        var("LOCALAPPDATA")
            .unwrap_or_else(std::env::temp_dir)
            .join("Znimok")
    } else if cfg!(target_os = "macos") {
        var("HOME")
            .unwrap_or_else(std::env::temp_dir)
            .join("Library/Application Support/Znimok")
    } else {
        var("XDG_RUNTIME_DIR")
            .unwrap_or_else(std::env::temp_dir)
            .join("znimok")
    }
}

impl Config {
    fn dir(&self) -> PathBuf {
        self.dir.clone().unwrap_or_else(default_dir)
    }
    fn tag(&self) -> String {
        self.suffix
            .as_deref()
            .map(|s| format!("-{s}"))
            .unwrap_or_default()
    }
    pub fn token_path(&self) -> PathBuf {
        self.dir().join(format!("ipc-token{}", self.tag()))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl RpcError {
    pub const PARSE: i64 = -32700;
    pub const INVALID: i64 = -32600;
    pub const NO_METHOD: i64 = -32601;
    pub const PARAMS: i64 = -32602;
    pub const INTERNAL: i64 = -32603;
    pub const UNAUTHORIZED: i64 = -32001;
    pub const TOO_LARGE: i64 = -32002;

    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
    pub fn method_not_found(m: &str) -> Self {
        Self::new(Self::NO_METHOD, format!("method not found: {m}"))
    }
    pub fn invalid_params(msg: impl Into<String>) -> Self {
        Self::new(Self::PARAMS, msg)
    }
    fn to_json(&self) -> Value {
        let mut e = json!({"code": self.code, "message": self.message});
        if let Some(d) = &self.data {
            e["data"] = d.clone();
        }
        e
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for RpcError {}

/// What the server does with an authenticated request. Called on the connection's thread.
pub trait Handler: Send + Sync + 'static {
    fn call(&self, method: &str, params: Value) -> Result<Value, RpcError>;
}

impl<F> Handler for F
where
    F: Fn(&str, Value) -> Result<Value, RpcError> + Send + Sync + 'static,
{
    fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        self(method, params)
    }
}

fn new_token() -> std::io::Result<String> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// Constant-time comparison, so the token cannot be guessed byte by byte from timing.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// One connection split into its halves, plus a way to bound the next read.
pub(crate) struct Split {
    pub read: Box<dyn Read + Send>,
    pub write: Box<dyn Write + Send>,
    /// `Some(t)`: the next read gives up after `t` (the connection is closed); `None`: no limit.
    pub timeout: Box<dyn Fn(Option<Duration>) + Send>,
}

/// Read one line of at most `max` bytes (without the newline). `Ok(None)` = the peer closed.
fn read_line<R: BufRead>(r: &mut R, max: usize) -> Result<Option<String>, RpcError> {
    let mut buf = Vec::new();
    let mut limited = r.take(max as u64 + 1);
    match limited.read_until(b'\n', &mut buf) {
        Ok(0) => return Ok(None),
        Ok(_) => {}
        Err(_) => return Ok(None),
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
    } else if buf.len() > max {
        return Err(RpcError::new(
            RpcError::TOO_LARGE,
            format!("request larger than {max} bytes"),
        ));
    }
    String::from_utf8(buf)
        .map(Some)
        .map_err(|_| RpcError::new(RpcError::PARSE, "not UTF-8"))
}

fn reply(w: &mut impl Write, id: &Value, result: Result<Value, RpcError>) -> std::io::Result<()> {
    let msg = match result {
        Ok(v) => json!({"jsonrpc": "2.0", "id": id, "result": v}),
        Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e.to_json()}),
    };
    let mut line = serde_json::to_vec(&msg)?;
    line.push(b'\n');
    w.write_all(&line)?;
    w.flush()
}

/// One connection: hello with the token first, then requests until the peer closes or idles out.
pub(crate) fn serve(conn: Split, token: &str, cfg: &Config, handler: &dyn Handler) {
    let Split {
        read,
        mut write,
        timeout,
    } = conn;
    let mut reader = BufReader::new(read);
    timeout(Some(cfg.hello_timeout));
    let mut authed = false;
    loop {
        if authed {
            timeout(Some(cfg.idle_timeout));
        }
        let line = match read_line(&mut reader, cfg.max_request) {
            Ok(Some(l)) => l,
            Ok(None) => return,
            Err(e) => {
                let _ = reply(&mut write, &Value::Null, Err(e));
                return;
            }
        };
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = reply(
                    &mut write,
                    &Value::Null,
                    Err(RpcError::new(RpcError::PARSE, e.to_string())),
                );
                if authed {
                    continue;
                }
                return;
            }
        };
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let Some(method) = req
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            let _ = reply(
                &mut write,
                &id,
                Err(RpcError::new(RpcError::INVALID, "no method")),
            );
            if authed {
                continue;
            }
            return;
        };
        let params = req.get("params").cloned().unwrap_or(Value::Null);
        // No deadline while the request is handled and answered.
        timeout(None);
        if !authed {
            let ok = method == "hello"
                && params
                    .get("token")
                    .and_then(Value::as_str)
                    .is_some_and(|t| same(t, token));
            if !ok {
                let _ = reply(
                    &mut write,
                    &id,
                    Err(RpcError::new(
                        RpcError::UNAUTHORIZED,
                        "hello with the session token first",
                    )),
                );
                return;
            }
            authed = true;
            let hello = json!({"server": "znimok", "version": env!("CARGO_PKG_VERSION"), "protocol": PROTOCOL});
            if reply(&mut write, &id, Ok(hello)).is_err() {
                return;
            }
            continue;
        }
        let result = handler.call(&method, params);
        if id.is_null() {
            continue; // a notification: no reply
        }
        if reply(&mut write, &id, result).is_err() {
            return;
        }
    }
}

/// A running server; dropping it stops accepting and removes the token file.
pub struct Server {
    stop: Arc<AtomicBool>,
    endpoint: String,
    token_path: PathBuf,
    #[cfg(windows)]
    _inner: win::Listener,
    #[cfg(unix)]
    _inner: unix::Listener,
}

impl Server {
    pub fn start(cfg: Config, handler: impl Handler) -> std::io::Result<Self> {
        let handler: Arc<dyn Handler> = Arc::new(handler);
        let dir = cfg.dir();
        std::fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let token = new_token()?;
        let token_path = cfg.token_path();
        write_private(&token_path, token.as_bytes())?;
        let stop = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicUsize::new(0));
        #[cfg(windows)]
        let inner = win::Listener::start(cfg.clone(), token, handler, stop.clone(), active)?;
        #[cfg(unix)]
        let inner = unix::Listener::start(cfg.clone(), token, handler, stop.clone(), active)?;
        Ok(Self {
            stop,
            endpoint: inner.endpoint(),
            token_path,
            _inner: inner,
        })
    }

    /// Pipe name or socket path.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = std::fs::remove_file(&self.token_path);
    }
}

/// Write a file only the current user can read (unix 0600; on Windows the per-user profile folder
/// already carries that ACL).
fn write_private(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("part");
    {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let mut f = o.open(&tmp)?;
        f.write_all(data)?;
    }
    std::fs::rename(&tmp, path)
}

#[derive(Debug)]
pub enum CallError {
    /// No server, or the connection broke.
    Io(std::io::Error),
    /// The server answered with an error.
    Rpc(RpcError),
    /// The answer was not valid JSON-RPC.
    Protocol(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IPC: {e}"),
            Self::Rpc(e) => write!(f, "IPC: {e}"),
            Self::Protocol(m) => write!(f, "IPC: {m}"),
        }
    }
}

impl std::error::Error for CallError {}

impl From<std::io::Error> for CallError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

pub struct Client {
    reader: BufReader<Box<dyn Read + Send>>,
    writer: Box<dyn Write + Send>,
    next_id: u64,
}

impl Client {
    /// Connect to the running server and say hello with the token from its token file.
    pub fn connect(cfg: &Config, client: &str) -> Result<Self, CallError> {
        let token = std::fs::read_to_string(cfg.token_path())?;
        #[cfg(windows)]
        let stream = win::connect(cfg)?;
        #[cfg(unix)]
        let stream = unix::connect(cfg)?;
        let mut c = Self {
            reader: BufReader::new(stream.0),
            writer: stream.1,
            next_id: 1,
        };
        c.call("hello", json!({"token": token.trim(), "client": client}))?;
        Ok(c)
    }

    /// Connect with an explicit token (tests of refusal).
    #[doc(hidden)]
    pub fn connect_with_token(cfg: &Config, token: &str) -> Result<Self, CallError> {
        #[cfg(windows)]
        let stream = win::connect(cfg)?;
        #[cfg(unix)]
        let stream = unix::connect(cfg)?;
        let mut c = Self {
            reader: BufReader::new(stream.0),
            writer: stream.1,
            next_id: 1,
        };
        c.call("hello", json!({"token": token, "client": "test"}))?;
        Ok(c)
    }

    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, CallError> {
        let id = self.next_id;
        self.next_id += 1;
        let mut line = serde_json::to_vec(
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )
        .map_err(|e| CallError::Protocol(e.to_string()))?;
        line.push(b'\n');
        self.writer.write_all(&line)?;
        self.writer.flush()?;
        let mut resp = String::new();
        if self.reader.read_line(&mut resp)? == 0 {
            return Err(CallError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "server closed",
            )));
        }
        let v: Value =
            serde_json::from_str(&resp).map_err(|e| CallError::Protocol(e.to_string()))?;
        if let Some(e) = v.get("error") {
            return Err(CallError::Rpc(RpcError {
                code: e.get("code").and_then(Value::as_i64).unwrap_or(0),
                message: e
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
                data: e.get("data").cloned(),
            }));
        }
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    }

    /// Send raw bytes (tests of framing limits).
    #[doc(hidden)]
    pub fn send_raw(&mut self, bytes: &[u8]) -> Result<String, CallError> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        let mut resp = String::new();
        self.reader.read_line(&mut resp)?;
        Ok(resp)
    }
}

#[cfg(test)]
mod tests;

use super::*;
use std::sync::atomic::AtomicU32;

fn cfg(tag: &str) -> Config {
    let dir = std::env::temp_dir().join(format!("znimok-ipc-{tag}-{}", std::process::id()));
    Config {
        suffix: Some(format!("test-{tag}-{}", std::process::id())),
        dir: Some(dir),
        ..Config::default()
    }
}

fn echo() -> impl Handler {
    |method: &str, params: Value| match method {
        "echo" => Ok(params),
        "fail" => Err(RpcError::invalid_params("bad")),
        _ => Err(RpcError::method_not_found(method)),
    }
}

#[test]
fn round_trip_errors_and_notifications() {
    let c = cfg("rt");
    let _s = Server::start(c.clone(), echo()).unwrap();
    let mut cl = Client::connect(&c, "test").unwrap();
    assert_eq!(
        cl.call("echo", json!({"a": [1, 2, "ї"]})).unwrap(),
        json!({"a": [1, 2, "ї"]})
    );
    match cl.call("fail", Value::Null) {
        Err(CallError::Rpc(e)) => assert_eq!(e.code, RpcError::PARAMS),
        other => panic!("{other:?}"),
    }
    match cl.call("nope", Value::Null) {
        Err(CallError::Rpc(e)) => assert_eq!(e.code, RpcError::NO_METHOD),
        other => panic!("{other:?}"),
    }
    // The connection survives errors.
    assert_eq!(cl.call("echo", json!(5)).unwrap(), json!(5));
}

#[test]
fn wrong_or_missing_token_is_refused_and_closed() {
    let c = cfg("tok");
    let _s = Server::start(c.clone(), echo()).unwrap();
    match Client::connect_with_token(&c, "00") {
        Err(CallError::Rpc(e)) => assert_eq!(e.code, RpcError::UNAUTHORIZED),
        other => panic!("{:?}", other.map(|_| ())),
    }
    // Calling anything before hello is refused too.
    #[cfg(windows)]
    let (r, w) = win::connect(&c).unwrap();
    #[cfg(unix)]
    let (r, w) = unix::connect(&c).unwrap();
    let mut cl = Client {
        reader: BufReader::new(r),
        writer: w,
        next_id: 1,
    };
    match cl.call("echo", json!(1)) {
        Err(CallError::Rpc(e)) => assert_eq!(e.code, RpcError::UNAUTHORIZED),
        other => panic!("{other:?}"),
    }
    assert!(
        cl.call("echo", json!(1)).is_err(),
        "connection must be closed after refusal"
    );
}

#[test]
fn oversized_request_is_refused() {
    let c = Config {
        max_request: 1000,
        ..cfg("big")
    };
    let _s = Server::start(c.clone(), echo()).unwrap();
    let mut cl = Client::connect(&c, "test").unwrap();
    let big = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"echo\",\"params\":\"{}\"}}\n",
        "x".repeat(5000)
    );
    let resp = cl.send_raw(big.as_bytes()).unwrap();
    assert!(resp.contains("-32002"), "{resp}");
}

#[test]
fn silent_client_is_dropped_after_hello_timeout() {
    let c = Config {
        hello_timeout: Duration::from_millis(300),
        ..cfg("slow")
    };
    let _s = Server::start(c.clone(), echo()).unwrap();
    #[cfg(windows)]
    let (mut r, _w) = win::connect(&c).unwrap();
    #[cfg(unix)]
    let (mut r, _w) = unix::connect(&c).unwrap();
    let t0 = std::time::Instant::now();
    let mut buf = [0u8; 16];
    let n = r.read(&mut buf).unwrap_or(0); // server closes: EOF or error
    assert_eq!(n, 0);
    assert!(t0.elapsed() < Duration::from_secs(3), "{:?}", t0.elapsed());
}

#[test]
fn several_clients_in_parallel() {
    let c = cfg("par");
    let calls = Arc::new(AtomicU32::new(0));
    let k = calls.clone();
    let _s = Server::start(c.clone(), move |_: &str, p: Value| {
        k.fetch_add(1, Ordering::SeqCst);
        Ok(p)
    })
    .unwrap();
    let hs: Vec<_> = (0..4)
        .map(|i| {
            let c = c.clone();
            std::thread::spawn(move || {
                let mut cl = Client::connect(&c, "t").unwrap();
                for j in 0..10 {
                    assert_eq!(cl.call("x", json!([i, j])).unwrap(), json!([i, j]));
                }
            })
        })
        .collect();
    for h in hs {
        h.join().unwrap();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 40);
}

#[test]
fn second_server_on_the_same_endpoint_fails_and_token_file_goes_away() {
    let c = cfg("dup");
    let s = Server::start(c.clone(), echo()).unwrap();
    assert!(
        Server::start(c.clone(), echo()).is_err(),
        "the endpoint must not be shared"
    );
    assert!(c.token_path().exists());
    drop(s);
    assert!(!c.token_path().exists());
}

#[cfg(windows)]
#[test]
fn pipe_admits_only_the_current_user() {
    let c = cfg("dacl");
    let _s = Server::start(c.clone(), echo()).unwrap();
    let sddl = win::endpoint_dacl(&c).unwrap();
    let sid = win::user_sid().unwrap();
    // Protected DACL with exactly one allow entry: this user, full access. Windows writes some
    // well-known accounts by alias (the built-in Administrator, RID 500, is "LA" — as on CI).
    let trustee = sddl
        .strip_prefix("D:P(A;;FA;;;")
        .and_then(|t| t.strip_suffix(')'))
        .unwrap_or_else(|| panic!("not a single allow entry: {sddl}"));
    let alias_ok = trustee == "LA" && sid.ends_with("-500");
    assert!(trustee == sid || alias_ok, "{sddl} vs {sid}");
}

#[cfg(unix)]
#[test]
fn socket_and_folder_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let c = cfg("perm");
    let s = Server::start(c.clone(), echo()).unwrap();
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(std::path::Path::new(s.endpoint())), 0o600);
    assert_eq!(mode(&c.dir()), 0o700);
    assert_eq!(mode(&c.token_path()), 0o600);
}

#[test]
fn flooding_client_is_slowed_down_not_served() {
    let c = Config {
        rate: 5,
        burst: 5,
        ..cfg("rate")
    };
    let _s = Server::start(c.clone(), echo()).unwrap();
    let mut cl = Client::connect(&c, "flood").unwrap();
    let mut ok = 0;
    let mut limited = None;
    for i in 0..20 {
        match cl.call("echo", json!(i)) {
            Ok(_) => ok += 1,
            Err(CallError::Rpc(e)) if e.code == RpcError::RATE_LIMITED => {
                limited.get_or_insert(e);
            }
            other => panic!("{other:?}"),
        }
    }
    assert!((5..=7).contains(&ok), "burst of 5 (+ refill): {ok}");
    let e = limited.expect("some requests refused");
    let wait = e.data.unwrap()["retry_after_ms"].as_u64().unwrap();
    assert!((1..=400).contains(&wait), "{wait}");
    // After waiting, the same connection works again.
    std::thread::sleep(Duration::from_millis(wait + 50));
    assert_eq!(cl.call("echo", json!("again")).unwrap(), json!("again"));
}

#[test]
fn repeated_wrong_tokens_are_answered_slowly_and_counted() {
    let c = Config {
        auth_failures_before_delay: 2,
        auth_delay: Duration::from_millis(400),
        ..cfg("guess")
    };
    let s = Server::start(c.clone(), echo()).unwrap();
    let t = |_| {
        let t0 = std::time::Instant::now();
        assert!(Client::connect_with_token(&c, "ff").is_err());
        t0.elapsed()
    };
    let times: Vec<_> = (0..4).map(t).collect();
    assert!(times[0] < Duration::from_millis(300), "{times:?}");
    assert!(times[1] < Duration::from_millis(300), "{times:?}");
    assert!(times[2] >= Duration::from_millis(390), "{times:?}");
    assert!(times[3] >= Duration::from_millis(390), "{times:?}");
    assert_eq!(s.auth_failures(), 4);
    // The right token is not held back.
    let t0 = std::time::Instant::now();
    Client::connect(&c, "me").unwrap();
    assert!(t0.elapsed() < Duration::from_millis(300));
}

#[test]
fn open_endpoint_reaches_our_own_server() {
    let c = cfg("open");
    let s = Server::start(c.clone(), echo()).unwrap();
    open_endpoint(s.endpoint()).unwrap();
    assert!(open_endpoint(&format!("{}-nope", s.endpoint())).is_err());
}

#[test]
fn a_second_start_hands_files_to_the_running_instance() {
    let c = cfg("fwd");
    assert!(
        !forward_open(&c, &[PathBuf::from("a.znimok")]).unwrap(),
        "nobody runs yet"
    );
    let got = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let g = got.clone();
    let _s = Server::start(c.clone(), move |m: &str, p: Value| {
        assert_eq!(m, OPEN_METHOD);
        let mut v = g.lock().unwrap();
        for x in p["paths"].as_array().unwrap() {
            v.push(x.as_str().unwrap().to_string());
        }
        Ok(Value::Null)
    })
    .unwrap();
    assert!(forward_open(&c, &[PathBuf::from("знімок 1.znimok")]).unwrap());
    let v = got.lock().unwrap();
    assert_eq!(v.len(), 1);
    assert!(std::path::Path::new(&v[0]).is_absolute(), "{v:?}");
    assert!(v[0].ends_with("знімок 1.znimok"), "{v:?}");
}

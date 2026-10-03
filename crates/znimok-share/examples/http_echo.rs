//! The OS HTTP stack's general request against a public echo service (ZK-101): a multipart form
//! with a Cyrillic file name, a PUT with JSON, raw bytes — on Windows (Windows.Web.Http) and macOS
//! (NSURLSession). `cargo run -p znimok-share --example http_echo`.

use std::time::Duration;

fn main() {
    let t = znimok_models::http::system();
    let form = "--b0\r\nContent-Disposition: form-data; name=\"chat_id\"\r\n\r\n42\r\n--b0\r\nContent-Disposition: form-data; name=\"document\"; filename=\"a.png\"; filename*=UTF-8''%D0%97.png\r\nContent-Type: image/png\r\n\r\nPNG\r\n--b0--\r\n";
    let r = t
        .request(
            "POST",
            "https://httpbin.org/anything",
            &[("X-Test", "1")],
            Some((form.as_bytes(), "multipart/form-data; boundary=b0")),
            Duration::from_secs(30),
            1 << 20,
        )
        .expect("POST");
    let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap_or_default();
    println!(
        "POST multipart: {} form={} files={} header={}",
        r.status, v["form"], v["files"], v["headers"]["X-Test"]
    );
    let r = t
        .request(
            "PUT",
            "https://httpbin.org/anything",
            &[],
            Some((br#"{"a":1}"#, "application/json")),
            Duration::from_secs(30),
            1 << 20,
        )
        .expect("PUT");
    let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap_or_default();
    println!(
        "PUT json: {} method={} json={}",
        r.status, v["method"], v["json"]
    );
    let r = t
        .request(
            "POST",
            "https://httpbin.org/anything",
            &[],
            Some((&[0u8, 1, 2, 255], "application/octet-stream")),
            Duration::from_secs(30),
            1 << 20,
        )
        .expect("POST bytes");
    let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap_or_default();
    println!(
        "POST bytes: {} data={} ct={}",
        r.status, v["data"], v["headers"]["Content-Type"]
    );
    let r = t
        .request(
            "GET",
            "https://httpbin.org/status/503",
            &[],
            None,
            Duration::from_secs(30),
            1 << 20,
        )
        .expect("GET");
    println!("GET 503: {}", r.status);
}

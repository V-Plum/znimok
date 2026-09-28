//! Windows.Web.Http.HttpClient (WinRT): system proxy and certificate store, HTTP/2.

use super::{HttpError, Response, Transport, retry_after};
use std::time::{Duration, Instant};
use windows::Foundation::Uri;
use windows::Storage::Streams::UnicodeEncoding;
use windows::Web::Http::{HttpClient, HttpMethod, HttpRequestMessage, HttpStringContent};
use windows::core::HSTRING;
use windows_future::AsyncStatus;

pub struct WinHttp;

fn net(e: windows::core::Error) -> HttpError {
    HttpError::Network(e.message())
}

impl Transport for WinHttp {
    fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        timeout: Duration,
    ) -> Result<Response, HttpError> {
        crate::ocr::winrt_thread();
        let body = std::str::from_utf8(body)
            .map_err(|_| HttpError::Network("body is not UTF-8".into()))?;
        let client = HttpClient::new().map_err(net)?;
        let req = HttpRequestMessage::Create(
            &HttpMethod::Post().map_err(net)?,
            &Uri::CreateUri(&HSTRING::from(url)).map_err(net)?,
        )
        .map_err(net)?;
        let h = req.Headers().map_err(net)?;
        for (k, v) in headers {
            h.TryAppendWithoutValidation(&HSTRING::from(*k), &HSTRING::from(*v))
                .map_err(net)?;
        }
        let content = HttpStringContent::CreateFromStringWithEncodingAndMediaType(
            &HSTRING::from(body),
            UnicodeEncoding::Utf8,
            &HSTRING::from("application/json"),
        )
        .map_err(net)?;
        req.SetContent(&content).map_err(net)?;

        let op = client.SendRequestAsync(&req).map_err(net)?;
        let t0 = Instant::now();
        // HttpClient has no timeout of its own: cancel the operation.
        while op.Status().map_err(net)? == AsyncStatus::Started {
            if t0.elapsed() > timeout {
                let _ = op.Cancel();
                return Err(HttpError::Timeout);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let resp = op.GetResults().map_err(net)?;
        let status = resp.StatusCode().map_err(net)?.0 as u16;
        let retry = resp
            .Headers()
            .ok()
            .and_then(|h| h.Lookup(&HSTRING::from("retry-after")).ok())
            .map(|v| v.to_string());
        let text = resp
            .Content()
            .map_err(net)?
            .ReadAsStringAsync()
            .map_err(net)?
            .join()
            .map_err(net)?;
        Ok(Response {
            status,
            body: text.to_string().into_bytes(),
            retry_after: retry_after(retry),
        })
    }
}

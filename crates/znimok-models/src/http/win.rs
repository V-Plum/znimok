//! Windows.Web.Http.HttpClient (WinRT): system proxy and certificate store, HTTP/2.

use super::{HttpError, Response, Transport, retry_after};
use std::time::{Duration, Instant};
use windows::Foundation::Uri;
use windows::Storage::Streams::{DataReader, DataWriter, UnicodeEncoding};
use windows::Web::Http::Headers::HttpMediaTypeHeaderValue;
use windows::Web::Http::{
    HttpBufferContent, HttpClient, HttpCompletionOption, HttpMethod, HttpRequestMessage,
    HttpStringContent,
};
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
            location: None,
        })
    }

    fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        timeout: Duration,
        max_bytes: usize,
    ) -> Result<Response, HttpError> {
        crate::ocr::winrt_thread();
        let client = HttpClient::new().map_err(net)?;
        let req = HttpRequestMessage::Create(
            &HttpMethod::Get().map_err(net)?,
            &Uri::CreateUri(&HSTRING::from(url)).map_err(net)?,
        )
        .map_err(net)?;
        let h = req.Headers().map_err(net)?;
        for (k, v) in headers {
            h.TryAppendWithoutValidation(&HSTRING::from(*k), &HSTRING::from(*v))
                .map_err(net)?;
        }
        let t0 = Instant::now();
        let wait = |status: &dyn Fn() -> windows::core::Result<AsyncStatus>,
                    cancel: &dyn Fn()|
         -> Result<(), HttpError> {
            while status().map_err(net)? == AsyncStatus::Started {
                if t0.elapsed() > timeout {
                    cancel();
                    return Err(HttpError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(())
        };
        // Headers first: the length is checked before the body is read.
        let op = client
            .SendRequestWithOptionAsync(&req, HttpCompletionOption::ResponseHeadersRead)
            .map_err(net)?;
        wait(&|| op.Status(), &|| {
            let _ = op.Cancel();
        })?;
        let resp = op.GetResults().map_err(net)?;
        let status = resp.StatusCode().map_err(net)?.0 as u16;
        let content = resp.Content().map_err(net)?;
        let announced = content
            .Headers()
            .and_then(|h| h.ContentLength())
            .and_then(|len| len.Value());
        if let Ok(n) = announced
            && n as usize > max_bytes
        {
            return Err(HttpError::Network(format!(
                "{n} bytes is more than {max_bytes}"
            )));
        }
        let read = content.ReadAsBufferAsync().map_err(net)?;
        wait(&|| read.Status(), &|| {
            let _ = read.Cancel();
        })?;
        let buf = read.GetResults().map_err(net)?;
        let n = buf.Length().map_err(net)? as usize;
        if n > max_bytes {
            return Err(HttpError::Network(format!(
                "{n} bytes is more than {max_bytes}"
            )));
        }
        let mut body = vec![0u8; n];
        DataReader::FromBuffer(&buf)
            .map_err(net)?
            .ReadBytes(&mut body)
            .map_err(net)?;
        Ok(Response {
            status,
            body,
            retry_after: None,
            location: None,
        })
    }

    fn request(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<(&[u8], &str)>,
        timeout: Duration,
        max_bytes: usize,
    ) -> Result<Response, HttpError> {
        crate::ocr::winrt_thread();
        let client = HttpClient::new().map_err(net)?;
        let m = HttpMethod::Create(&HSTRING::from(method)).map_err(net)?;
        let req =
            HttpRequestMessage::Create(&m, &Uri::CreateUri(&HSTRING::from(url)).map_err(net)?)
                .map_err(net)?;
        let h = req.Headers().map_err(net)?;
        for (k, v) in headers {
            h.TryAppendWithoutValidation(&HSTRING::from(*k), &HSTRING::from(*v))
                .map_err(net)?;
        }
        if let Some((bytes, ct)) = body {
            let w = DataWriter::new().map_err(net)?;
            w.WriteBytes(bytes).map_err(net)?;
            let content = HttpBufferContent::CreateFromBuffer(&w.DetachBuffer().map_err(net)?)
                .map_err(net)?;
            // A content header, not a request header (multipart carries its boundary in it).
            content
                .Headers()
                .map_err(net)?
                .SetContentType(&HttpMediaTypeHeaderValue::Parse(&HSTRING::from(ct)).map_err(net)?)
                .map_err(net)?;
            req.SetContent(&content).map_err(net)?;
        }
        let t0 = Instant::now();
        let wait = |status: &dyn Fn() -> windows::core::Result<AsyncStatus>,
                    cancel: &dyn Fn()|
         -> Result<(), HttpError> {
            while status().map_err(net)? == AsyncStatus::Started {
                if t0.elapsed() > timeout {
                    cancel();
                    return Err(HttpError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(())
        };
        let op = client.SendRequestAsync(&req).map_err(net)?;
        wait(&|| op.Status(), &|| {
            let _ = op.Cancel();
        })?;
        let resp = op.GetResults().map_err(net)?;
        let status = resp.StatusCode().map_err(net)?.0 as u16;
        let retry = resp
            .Headers()
            .ok()
            .and_then(|h| h.Lookup(&HSTRING::from("retry-after")).ok())
            .map(|v| v.to_string());
        let location = resp
            .Headers()
            .ok()
            .and_then(|h| h.Lookup(&HSTRING::from("location")).ok())
            .map(|v| v.to_string());
        let read = resp
            .Content()
            .map_err(net)?
            .ReadAsBufferAsync()
            .map_err(net)?;
        wait(&|| read.Status(), &|| {
            let _ = read.Cancel();
        })?;
        let buf = read.GetResults().map_err(net)?;
        let n = buf.Length().map_err(net)? as usize;
        if n > max_bytes {
            return Err(HttpError::Network(format!(
                "{n} bytes is more than {max_bytes}"
            )));
        }
        let mut out = vec![0u8; n];
        DataReader::FromBuffer(&buf)
            .map_err(net)?
            .ReadBytes(&mut out)
            .map_err(net)?;
        Ok(Response {
            status,
            body: out,
            retry_after: retry_after(retry),
            location,
        })
    }
}

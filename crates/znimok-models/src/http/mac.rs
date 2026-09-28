//! NSURLSession (shared session): system proxy, keychain trust, ATS.

use super::{HttpError, Response, Transport, retry_after};
use block2::RcBlock;
use objc2_foundation::{
    NSData, NSError, NSHTTPURLResponse, NSMutableURLRequest, NSString, NSURL, NSURLResponse,
    NSURLSession,
};
use std::sync::mpsc;
use std::time::Duration;

pub struct MacHttp;

type Outcome = Result<(u16, Vec<u8>, Option<String>), HttpError>;

impl Transport for MacHttp {
    fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        timeout: Duration,
    ) -> Result<Response, HttpError> {
        let url = NSURL::URLWithString(&NSString::from_str(url))
            .ok_or_else(|| HttpError::Network("bad URL".into()))?;
        let req = NSMutableURLRequest::requestWithURL(&url);
        req.setHTTPMethod(&NSString::from_str("POST"));
        req.setTimeoutInterval(timeout.as_secs_f64());
        let set = |k: &str, v: &str| {
            req.setValue_forHTTPHeaderField(Some(&NSString::from_str(v)), &NSString::from_str(k))
        };
        set("content-type", "application/json");
        for (k, v) in headers {
            set(k, v);
        }
        req.setHTTPBody(Some(&NSData::with_bytes(body)));

        let (tx, rx) = mpsc::channel::<Outcome>();
        let block = RcBlock::new(
            move |data: *mut NSData, resp: *mut NSURLResponse, error: *mut NSError| {
                // SAFETY: the pointers are valid (or null) for the duration of the call.
                let outcome = unsafe {
                    if let Some(e) = error.as_ref() {
                        Err(if e.code() == -1001 {
                            HttpError::Timeout
                        } else {
                            HttpError::Network(e.localizedDescription().to_string())
                        })
                    } else {
                        let http = resp
                            .as_ref()
                            .and_then(|r| r.downcast_ref::<NSHTTPURLResponse>());
                        match http {
                            None => Err(HttpError::Network("not an HTTP response".into())),
                            Some(h) => Ok((
                                h.statusCode() as u16,
                                data.as_ref().map(|d| d.to_vec()).unwrap_or_default(),
                                h.valueForHTTPHeaderField(&NSString::from_str("retry-after"))
                                    .map(|s| s.to_string()),
                            )),
                        }
                    }
                };
                let _ = tx.send(outcome);
            },
        );
        let session = NSURLSession::sharedSession();
        let task = session.dataTaskWithRequest_completionHandler(&req, &block);
        task.resume();
        match rx.recv_timeout(timeout + Duration::from_secs(5)) {
            Ok(Ok((status, body, retry))) => Ok(Response {
                status,
                body,
                retry_after: retry_after(retry),
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                task.cancel();
                Err(HttpError::Timeout)
            }
        }
    }
}

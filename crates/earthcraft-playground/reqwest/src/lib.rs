// SPDX-License-Identifier: MIT
//! The WASM subset of reqwest used by earthcraft-data. The Desktop host
//! fetches allowlisted imagery and returns plain bytes to the data tasks.
//! A timeout covers the whole request. The caller receives the body in one
//! chunk, and its usual size limit still applies.
#![cfg(target_arch = "wasm32")]

use bytes::Bytes;
pub use reqwest::{Method, StatusCode};
use std::time::Duration;

/// A request that failed, or came back with an error status.
#[derive(Debug)]
pub struct Error {
    message: String,
    status: Option<StatusCode>,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
impl Error {
    fn new(message: impl ToString) -> Self {
        Self {
            message: message.to_string(),
            status: None,
        }
    }
    pub fn status(&self) -> Option<StatusCode> {
        self.status
    }
    pub fn is_status(&self) -> bool {
        self.status.is_some()
    }
}
impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Self {
            status: error.status(),
            message: error.to_string(),
        }
    }
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Default)]
pub struct Client {
    timeout: Option<Duration>,
}
impl Client {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }
    pub fn get(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::GET, url)
    }
    pub fn post(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::POST, url)
    }
    pub fn request(&self, method: Method, url: impl AsRef<str>) -> RequestBuilder {
        RequestBuilder {
            method,
            url: url.as_ref().into(),
            headers: Vec::new(),
            body: None,
            timeout: self.timeout,
            error: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct ClientBuilder {
    timeout: Option<Duration>,
    read_timeout: Option<Duration>,
}
impl ClientBuilder {
    pub fn user_agent<V>(self, _: V) -> Self {
        self
    }
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
    pub fn connect_timeout(self, _: Duration) -> Self {
        self
    }
    pub fn read_timeout(mut self, timeout: Duration) -> Self {
        self.read_timeout = Some(timeout);
        self
    }
    pub fn no_proxy(self) -> Self {
        self
    }
    pub fn build(self) -> Result<Client> {
        Ok(Client {
            timeout: self.timeout.or(self.read_timeout),
        })
    }
}

#[derive(Debug)]
pub struct RequestBuilder {
    method: Method,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    timeout: Option<Duration>,
    error: Option<String>,
}
impl RequestBuilder {
    pub fn header(mut self, name: impl AsRef<str>, value: impl AsRef<str>) -> Self {
        self.headers
            .push((name.as_ref().into(), value.as_ref().into()));
        self
    }
    pub fn query<T: serde::Serialize + ?Sized>(mut self, query: &T) -> Self {
        match serde_urlencoded::to_string(query) {
            Ok(q) if !q.is_empty() => {
                self.url
                    .push(if self.url.contains('?') { '&' } else { '?' });
                self.url.push_str(&q);
            }
            Ok(_) => {}
            Err(error) => self.error = Some(error.to_string()),
        }
        self
    }
    pub fn form<T: serde::Serialize + ?Sized>(mut self, form: &T) -> Self {
        match serde_urlencoded::to_string(form) {
            Ok(body) => {
                self.body = Some(body.into_bytes());
                self.headers.push((
                    "Content-Type".into(),
                    "application/x-www-form-urlencoded".into(),
                ));
            }
            Err(error) => self.error = Some(error.to_string()),
        }
        self
    }
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(body.into());
        self
    }
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
    /// Fetches on the page's event loop; the answer comes back whole.
    pub async fn send(self) -> Result<Response> {
        if let Some(error) = self.error {
            return Err(Error::new(error));
        }
        // The WebOS viewer permits this imagery origin. Desktop services use the native client.
        if self.method != Method::GET
            || !self
                .url
                .starts_with("https://gibs.earthdata.nasa.gov/wmts/")
        {
            return Err(Error::new(
                "This data source is unavailable in the WebOS browser.",
            ));
        }
        let (answer, response) = tokio::sync::oneshot::channel();
        let url = self.url.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let _ = answer.send(self.fetch().await);
        });
        response
            .await
            .map_err(|_| Error::new(format!("request for {url} was dropped")))?
    }

    async fn fetch(&self) -> Result<Response> {
        use js_sys::{Function, Promise, Reflect, Uint8Array};
        use wasm_bindgen::{JsCast as _, JsValue};

        let get = |object: &JsValue, key: &str| Reflect::get(object, &key.into());
        let host = get(&js_sys::global(), "playgroundFiles")
            .map_err(|_| Error::new("The imagery host is unavailable."))?;
        let fetch = get(&host, "fetchImagery")
            .ok()
            .and_then(|value| value.dyn_into::<Function>().ok())
            .ok_or_else(|| Error::new("The imagery host is unavailable."))?;
        let timeout_ms = self.timeout.map_or(0, |timeout| {
            timeout.as_millis().min(u128::from(u32::MAX)) as u32
        });
        let promise = fetch
            .call2(&host, &self.url.as_str().into(), &timeout_ms.into())
            .ok()
            .and_then(|value| value.dyn_into::<Promise>().ok())
            .ok_or_else(|| Error::new("The imagery host could not start the request."))?;
        let result = wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(|error| {
                let message = get(&error, "message")
                    .ok()
                    .and_then(|value| value.as_string());
                Error::new(
                    message.unwrap_or_else(|| "Could not load the NASA imagery tile.".into()),
                )
            })?;
        let status = get(&result, "status")
            .ok()
            .and_then(|value| value.as_f64())
            .and_then(|code| StatusCode::from_u16(code as u16).ok())
            .ok_or_else(|| Error::new("The imagery host returned an invalid status."))?;
        let bytes = get(&result, "bytes")
            .ok()
            .filter(JsValue::is_object)
            .map(|value| Uint8Array::new(&value).to_vec())
            .ok_or_else(|| Error::new("The imagery host returned invalid tile data."))?;
        Ok(Response {
            status,
            url: self.url.clone(),
            body: Some(Bytes::from(bytes)),
        })
    }
}

#[derive(Debug)]
pub struct Response {
    status: StatusCode,
    url: String,
    body: Option<Bytes>,
}
impl Response {
    pub fn status(&self) -> StatusCode {
        self.status
    }
    pub fn error_for_status(self) -> Result<Self> {
        if self.status.is_client_error() || self.status.is_server_error() {
            return Err(Error {
                message: format!("HTTP status {} for url ({})", self.status, self.url),
                status: Some(self.status),
            });
        }
        Ok(self)
    }
    /// The whole body the first time, then None.
    pub async fn chunk(&mut self) -> Result<Option<Bytes>> {
        Ok(self.body.take())
    }
    pub async fn bytes(self) -> Result<Bytes> {
        Ok(self.body.unwrap_or_default())
    }
    pub async fn text(self) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.body.unwrap_or_default()).into_owned())
    }
    pub async fn json<T: serde::de::DeserializeOwned>(self) -> Result<T> {
        serde_json::from_slice(&self.body.unwrap_or_default()).map_err(Error::new)
    }
}

use std::sync::mpsc::Sender;

use hyper::{body::Bytes, HeaderMap, Method, Uri};

#[derive(Debug)]
pub enum ASGIMessages {
    HttpResponseStart(HttpResponseStart),
    HttpResponseBody(HttpResponseBody),
}

#[derive(Debug)]
pub struct HttpResponseBody {
    pub body: Vec<u8>,
}

impl HttpResponseBody {
    pub fn new(body: Vec<u8>) -> Self {
        Self { body }
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

#[derive(Debug)]
pub struct HttpResponseStart {
    pub response_type: String,
    pub status: u16,
    pub headers: Vec<(Vec<u8>, Vec<u8>)>,
    pub trailers: bool,
}

impl HttpResponseStart {
    pub fn new(response_type: &str, status: u16) -> Self {
        Self {
            response_type: response_type.to_string(),
            status,
            headers: Vec::new(),
            trailers: false,
        }
    }

    pub fn add_header(&mut self, name: &[u8], value: &[u8]) {
        self.headers.push((name.to_vec(), value.to_vec()));
    }

    pub fn set_trailers(&mut self, trailers: bool) {
        self.trailers = trailers;
    }
}

pub struct ParsedRequest {
    pub headers: HeaderMap,
    pub method: Method,
    pub uri: Uri,
    pub body: Bytes,
    pub callback: Sender<ASGIMessages>,
}

impl ParsedRequest {
    pub fn new(
        headers: HeaderMap,
        method: Method,
        uri: Uri,
        body: Bytes,
        callback: Sender<ASGIMessages>,
    ) -> Self {
        Self {
            headers,
            method,
            uri,
            body,
            callback,
        }
    }
}

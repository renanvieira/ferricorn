use std::{collections::HashMap, fmt::Display, sync::mpsc::Sender};

use hyper::{body::Bytes, HeaderMap, Method};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize,Deserialize)]
pub enum ASGIMessages {
    HttpResponseStart(HttpResponseStart),
    HttpResponseBody(HttpResponseBody),
}

#[derive(Debug, Serialize,Deserialize)]
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

#[derive(Debug, Serialize,Deserialize)]
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

#[derive(Serialize, Deserialize, Debug)]
pub enum HttpMethod {
    POST,
    GET,
    PATCH,
    OPTIONS,
    DELETE,
    HEAD,
}

impl Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let strs_val = match self {
            HttpMethod::POST => "POST",
            HttpMethod::GET => "GET",
            HttpMethod::PATCH => "PATCH",
            HttpMethod::OPTIONS => "OPTIONS",
            HttpMethod::DELETE => "DELETE",
            HttpMethod::HEAD => "HEAD",
        };

        f.write_str(strs_val)
    }
}

impl TryFrom<String> for HttpMethod {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.to_uppercase().as_ref() {
            "POST" => Ok(Self::POST),
            "GET" => Ok(Self::GET),
            "PATCH" => Ok(Self::PATCH),
            "OPTIONS" => Ok(Self::OPTIONS),
            "DELETE" => Ok(Self::DELETE),
            "HEAD" => Ok(Self::HEAD),
            _ => Err(format!("Invalid HTTPMethod: {}", value)),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Uri {
    scheme: Option<String>,
    path: String,
    query_string: Option<String>,
}

impl Uri {
    pub fn new(scheme: Option<String>, path: String, query_string: Option<String>) -> Self {
        Self {
            scheme,
            path,
            query_string,
        }
    }

    pub fn scheme(&self) -> Option<&str> {
        self.scheme.as_deref()
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn query_string(&self) -> Option<&str> {
        self.query_string.as_deref()
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ParsedRequest {
    pub headers: HashMap<String, String>,
    pub method: HttpMethod,
    pub body: Vec<u8>,
    pub uri: Uri,
}

impl ParsedRequest {
    pub fn new(
        headers: HashMap<String, String>,
        method: HttpMethod,
        body: Vec<u8>,
        uri: Uri,
    ) -> Self {
        Self {
            headers,
            method,
            body,
            uri,
        }
    }
}

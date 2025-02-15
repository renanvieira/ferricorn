mod py_process;
pub mod types;

use std::env;
use std::net::SocketAddr;
use std::sync::mpsc::Sender;

use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::header::{HeaderName, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{HeaderMap, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use py_process::PythonProcess;
use tokio::net::TcpListener;
use types::{ASGIMessages, ParsedRequest};

async fn process_request(
    req: Request<hyper::body::Incoming>,
    asgi_tx: Sender<ParsedRequest>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let headers = req.headers().to_owned();
    let method = req.method().to_owned();
    let uri = req.uri().to_owned();
    let body = req.collect().await.unwrap().to_bytes();
    let (resp_tx, resp_rx) = std::sync::mpsc::channel::<Response<Full<Bytes>>>();
    let (tx, rx) = std::sync::mpsc::channel::<ASGIMessages>();

    let request = ParsedRequest::new(headers, method, uri, body, tx);

    asgi_tx.send(request).unwrap();

    let join_handle = tokio::spawn(async move {
        let mut status_code = StatusCode::INTERNAL_SERVER_ERROR;
        let mut headers = HeaderMap::new();

        while let Ok(msg) = rx.recv() {
            match msg {
                ASGIMessages::HttpResponseStart(http_response_start) => {
                    dbg!(&http_response_start);
                    status_code = StatusCode::from_u16(http_response_start.status).unwrap();
                    for (n, v) in http_response_start.headers {
                        headers.insert(
                            HeaderName::from_bytes(&n).unwrap(),
                            HeaderValue::from_bytes(&v).unwrap(),
                        );
                    }
                }
                ASGIMessages::HttpResponseBody(http_response_body) => {
                    let mut response = Response::builder();
                    let headers_resp = response.headers_mut().unwrap();

                    for (k, v) in headers.clone() {
                        if let (Some(key), value) = (k, v) {
                            headers_resp.insert(key, value);
                        }
                    }

                    let response = response
                        .status(status_code)
                        .body(Full::from(Bytes::from(http_response_body.body)))
                        .unwrap();
                    resp_tx.send(response).unwrap();
                    break;
                }
            }
        }
    });

    join_handle.await.unwrap();

    Ok(resp_rx.recv().unwrap())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = env::args().collect();
    let (module, asgi_attr) = args[1].split_once(":").unwrap();

    let addr = SocketAddr::from(([127, 0, 0, 1], 3100));

    let listener = TcpListener::bind(addr).await?;
    let python_worker_tx = PythonProcess::start(module.to_owned(), asgi_attr.to_owned()).unwrap();

    loop {
        let (stream, _) = listener.accept().await?;
        let iteration_tx = python_worker_tx.clone();

        let io = TokioIo::new(stream);

        tokio::task::spawn(async move {
            let service = service_fn(move |req: Request<Incoming>| {
                let iteration_tx_clone = iteration_tx.clone();
                async move {
                    let response_result = process_request(req, iteration_tx_clone).await;
                    match response_result {
                        Ok(response) => Ok(response),
                        Err(err) => Err(err),
                    }
                }
            });

            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                eprintln!("Error serving connection: {:?}", err);
            }
        });
    }
}

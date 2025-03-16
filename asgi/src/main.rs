use std::collections::HashMap;
use std::io::{stderr, stdout};
use std::net::SocketAddr;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, LazyLock};
use std::{env, os};

use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::header::{HeaderName, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{HeaderMap, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use messages::types::{ASGIMessages, HttpMethod, ParsedRequest, Uri};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UnixStream};
use tokio::sync::Mutex;

async fn process_request(
    req: Request<hyper::body::Incoming>,
    sock_file: String,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let headers: HashMap<String, String> = req
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap().to_string()))
        .collect();

    let method = HttpMethod::try_from(req.method().to_string()).unwrap();
    let scheme = req.uri().scheme().map(|s| s.to_string());
    let path = req.uri().path().to_string();
    let query_string = req.uri().query().map(|str| str.to_string());

    let uri = Uri::new(scheme, path, query_string);
    let body = req.collect().await.unwrap().to_bytes().to_vec();

    let request = ParsedRequest::new(headers, method, body, uri);

    let mut stream = UnixStream::connect(sock_file).await.unwrap();

    let payload = bincode::serialize(&request).unwrap();
    dbg!("Sending payload", &request);
    stream
        .write_all(&(payload.len() as u32).to_be_bytes())
        .await
        .unwrap();
    stream.write_all(&payload).await.unwrap();
    dbg!(" payload sent  to worker");

    let mut status_code = StatusCode::INTERNAL_SERVER_ERROR;
    let mut headers = HeaderMap::new();

    loop {
        dbg!("waiting response");
        let mut buf_len = [0u8; 4];
        stream.read_exact(&mut buf_len).await.unwrap();

        let lenght = u32::from_be_bytes(buf_len) as usize;
        let mut payload_buf = vec![0u8; lenght];

        stream.read_exact(&mut payload_buf).await.unwrap();

        let msg = bincode::deserialize::<ASGIMessages>(&payload_buf).unwrap();

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
                let mut builder = Response::builder();
                let headers_resp = builder.headers_mut().unwrap();

                for (k, v) in headers.clone() {
                    if let (Some(key), value) = (k, v) {
                        headers_resp.insert(key, value);
                    }
                }

                return Ok(builder
                    .status(status_code)
                    .body(Full::new(Bytes::from(http_response_body.body)))
                    .unwrap());
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = env::args().collect();
    let addr = SocketAddr::from(([127, 0, 0, 1], 3100));
    let listener = TcpListener::bind(addr).await?;
    let worker_count = args.get(2).map_or("1", |v| v).parse::<usize>().unwrap();

    let sock_file = "/tmp/ferricorn_worker";
    let workers: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    for i in 0..worker_count {
        let module = args[1].clone();
        let sock_file = format!("{}_{}", sock_file, i);
        let workers = Arc::clone(&workers);

        tokio::spawn(async move {
            let mut child = Command::new("cargo")
                .args([
                    "run", "-p", "worker", "--", "--module", &module, "--sock", &sock_file,
                ])
                .stdout(stdout())
                .stderr(stderr())
                .spawn()
                .expect("couldn't start worker");

            dbg!(child.id());
            {
                let mut workers = workers.lock().await;
                workers.push(sock_file.clone());
            }
            let exit_code = child.wait().expect("worker not running");
            dbg!("child process exited with  {}", exit_code);

            {
                let mut workers = workers.lock().await;
                let position = workers
                    .iter()
                    .position(|w| *w == sock_file)
                    .expect("worker file not found in list of actrive workers");

                workers.remove(position);
            }
        });
    }

    loop {
        let (stream, _) = listener.accept().await?;

        let io = TokioIo::new(stream);
        let workers = Arc::clone(&workers);

        tokio::task::spawn(async move {
            let service = service_fn(move |req: Request<Incoming>| {
                let inner_workers = Arc::clone(&workers);

                async move {
                    let sock_file = round_robin(&inner_workers).await;
                    let response_result = process_request(req, sock_file.to_string()).await;
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

static NEXT_WORKER: LazyLock<Arc<Mutex<usize>>> = LazyLock::new(|| Arc::new(Mutex::new(0)));

async fn round_robin(workers: &Arc<Mutex<Vec<String>>>) -> String {
    let mut next_worker_index = NEXT_WORKER.lock().await;
    let workers = workers.lock().await;

    let next_worker = workers.get(*next_worker_index).unwrap();

    *next_worker_index += 1;

    if *next_worker_index >= workers.len() {
        *next_worker_index = 0;
    }

    next_worker.to_owned()
}

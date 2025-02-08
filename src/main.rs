pub mod types;

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::header::{HeaderName, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{HeaderMap, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use pyo3::ffi::PyObject;
use pyo3::types::{
    PyAny, PyAnyMethods, PyBytes, PyCFunction, PyDict, PyList, PyListMethods, PyModule, PyString,
    PyTuple,
};
use pyo3::{Bound, IntoPy, IntoPyObjectExt, Py, PyResult, Python};
use tokio::net::TcpListener;
use types::{ASGIMessages, HttpResponseBody, HttpResponseStart};

async fn hello(req: Request<hyper::body::Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    // coroutine application(scope, receive, send)
    let headers = req.headers().to_owned();
    let method = req.method().to_owned();
    let uri = req.uri().to_owned();

    let body = req.collect().await.unwrap().to_bytes();

    let (resp_tx, resp_rx) = std::sync::mpsc::channel::<Response<Full<Bytes>>>();
    let (tx, rx) = std::sync::mpsc::channel::<ASGIMessages>();
    let tx = Arc::new(tx);
    let tx_clone = Arc::clone(&tx);

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

    Python::with_gil(|py| {
        dbg!(" python started");
        let _ = py.check_signals();
        // Ensure Python's signal handlers are set up

        let sys_path = py.import("sys").unwrap().getattr("path").unwrap();
        let _ = sys_path
            .call_method1(
                "append",
                (format!(
                    "{}/py",
                    std::env::current_dir().unwrap().to_string_lossy()
                ),),
            )
            .unwrap();

        let app_module = PyModule::import(py, "echo_server").unwrap();

        let scope = PyDict::new(py);
        let _ = scope.set_item("type", "http");

        let asgi = PyDict::new(py);
        let _ = asgi.set_item("version", "3.0");
        let _ = asgi.set_item("spec_version", "2.1");

        let _ = scope.set_item("asgi", asgi);
        let _ = scope.set_item("http_version", "1.1");
        let _ = scope.set_item("method", method.as_str());
        if let Some(scheme) = uri.scheme() {
            let _ = scope.set_item("scheme", scheme.to_string().as_str());
        }
        let _ = scope.set_item("path", uri.path());
        let _ = scope.set_item("raw_path", uri.path().as_bytes());
        let _ = scope.set_item("query_string", uri.query());
        let _ = scope.set_item("root_path", "");

        let scope_headers = PyList::empty(py);

        for (name, val) in headers {
            let _ = scope_headers.append(
                PyTuple::new(py, [name.unwrap().as_str().as_bytes(), val.as_bytes()]).unwrap(),
            );
        }

        let _ = scope.set_item("headers", scope_headers);

        let _ = scope.set_item("client", "");
        let _ = scope.set_item("server", "");

        let clone_body = body.clone();
        let receive_callback = move |args: &Bound<'_, PyTuple>,
                                     _kwargs: Option<&Bound<'_, PyDict>>|
              -> PyResult<Py<PyAny>> {
            Python::with_gil(|py| {
                dbg!(args);
                dbg!(_kwargs);

                let callback_body = clone_body.clone();
                let body = PyBytes::new(py, &callback_body.to_vec());

                let event = PyDict::new(py);

                event.set_item("type", "http.request").unwrap();
                event.set_item("body", body).unwrap();
                event.set_item("more_body", false).unwrap();

                // Create and return a resolved future
                let asyncio = py.import("asyncio")?;
                let future = asyncio.call_method0("Future")?;
                future.call_method1("set_result", (event,))?;

                Ok(future.into())
            })
        };

        let tx_for_send = Arc::clone(&tx_clone);
        let send_callback = move |args: &Bound<'_, PyTuple>,
                                  _kwargs: Option<&Bound<'_, PyDict>>|
              -> PyResult<Py<PyAny>> {
            Python::with_gil(|py| {
                let data = args.get_item(0).unwrap();
                let data_type_result = data.get_item("type").unwrap();
                let data_type = data_type_result.extract::<String>().unwrap();

                let data_type_ref = data_type.as_str();

                match data_type_ref {
                    "http.response.start" => {
                        let start = HttpResponseStart::new(
                            &data_type,
                            data.get_item("status").unwrap().extract::<u16>().unwrap(),
                        );

                        dbg!(&start);
                        tx_for_send
                            .send(ASGIMessages::HttpResponseStart(start))
                            .unwrap();
                    }
                    "http.response.body" => {
                        let body_bytes = data.get_item("body").unwrap();
                        let body_vec = body_bytes.extract::<Vec<u8>>();

                        let body = HttpResponseBody::new(body_vec.unwrap());

                        tx_for_send
                            .send(ASGIMessages::HttpResponseBody(body))
                            .unwrap();
                    }
                    _ => {
                        dbg!(data);
                    }
                };

                // Create and return a resolved future
                let asyncio = py.import("asyncio")?;
                let future = asyncio.call_method0("Future")?;
                future.call_method1("set_result", (py.None(),))?;

                Ok(future.into())
            })
        };

        let receive = PyCFunction::new_closure(py, None, None, receive_callback).unwrap();
        let send = PyCFunction::new_closure(py, None, None, send_callback).unwrap();

        dbg!(&scope);
        let scope_any: Py<PyAny> = scope.into();
        let receive_any: Py<PyAny> = receive.into();
        let send_any: Py<PyAny> = send.into();

        let asgi_args = PyTuple::new(py, &[scope_any, receive_any, send_any]).unwrap();

        // Import asyncio to run the event loop
        let asyncio = PyModule::import(py, "asyncio").unwrap();
        // Create and set new event loop

        let event_loop = asyncio.call_method0("new_event_loop").unwrap();
        asyncio
            .call_method1("set_event_loop", (&event_loop,))
            .unwrap();

        let asgi_app = app_module.getattr("app").unwrap();
        // Create the coroutine to call the FastAPI app
        let coroutine = asgi_app.call1(asgi_args).unwrap();

        // Run the coroutine until it completes
        event_loop
            .call_method1("run_until_complete", (coroutine,))
            .unwrap();

        event_loop.call_method0("close").unwrap();
        let _ = py.check_signals();
    });

    drop(tx_clone);
    drop(tx);
    join_handle.await.unwrap();

    Ok(resp_rx.recv().unwrap())
    // Ok(Response::new(Full::new(Bytes::from("Hello, World!"))))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = SocketAddr::from(([127, 0, 0, 1], 3100));

    // We create a TcpListener and bind it to 127.0.0.1:3100
    let listener = TcpListener::bind(addr).await?;

    // We start a loop to continuously accept incoming connections
    loop {
        let (stream, _) = listener.accept().await?;

        // Use an adapter to access something implementing `tokio::io` traits as if they implement
        // `hyper::rt` IO traits.
        let io = TokioIo::new(stream);

        // Spawn a tokio task to serve multiple connections concurrently
        tokio::task::spawn(async move {
            // Finally, we bind the incoming connection to our `hello` service
            if let Err(err) = http1::Builder::new()
                // `service_fn` converts our function in a `Service`
                .serve_connection(io, service_fn(hello))
                .await
            {
                eprintln!("Error serving connection: {:?}", err);
            }
        });
    }
}

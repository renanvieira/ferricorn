use std::{
    io::Write as _,
    os::unix::net::UnixStream,
    sync::{mpsc, Arc},
    thread,
};

use crossbeam_channel::Sender;
use pyo3::{
    types::{
        PyAnyMethods, PyBytes, PyCFunction, PyDict, PyList, PyListMethods, PyModule, PyString,
        PyTuple,
    },
    Bound, IntoPyObjectExt, Py, PyAny, PyResult, Python,
};

use messages::types::{ASGIMessages, HttpResponseBody, HttpResponseStart, ParsedRequest};
use tokio::io::AsyncWriteExt;

pub struct PythonProcess;

impl PythonProcess {
    pub fn start(
        app_module: String,
        asgi_attr: String,
        asgi_sender: Sender<ASGIMessages>,
    ) -> Result<mpsc::Sender<ParsedRequest>, Box<dyn std::error::Error>> {
        let (tx, rx) = mpsc::channel::<ParsedRequest>();

        thread::spawn(move || {
            dbg!(" starting python");
            Python::with_gil(|py| {
                dbg!(" python started");
                let app_module_arc = Arc::new(app_module);
                let asgi_attr_arc = Arc::new(asgi_attr);

                while let Ok(request_data) = rx.recv() {
                    // Ensure Python's signal handlers are set up
                    let _ = py.check_signals();
                    let app_module_arc = Arc::clone(&app_module_arc);
                    let asgi_attr_arc = Arc::clone(&asgi_attr_arc);

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

                    let app_module =
                        PyModule::import(py, PyString::new(py, &app_module_arc)).unwrap();

                    let scope = PyDict::new(py);
                    let _ = scope.set_item("type", "http");

                    let asgi = PyDict::new(py);
                    let _ = asgi.set_item("version", "3.0");
                    let _ = asgi.set_item("spec_version", "2.1");

                    let _ = scope.set_item("asgi", asgi);
                    let _ = scope.set_item("http_version", "1.1");
                    let _ = scope.set_item("method", request_data.method.to_string());
                    let _ = scope.set_item("scheme", request_data.uri.scheme());
                    let _ = scope.set_item("path", request_data.uri.path());
                    let _ = scope.set_item("raw_path", request_data.uri.path().as_bytes());
                    let _ = scope.set_item("query_string", request_data.uri.query_string());
                    let _ = scope.set_item("root_path", "");

                    let scope_headers = PyList::empty(py);

                    for (name, val) in request_data.headers {
                        let _ = scope_headers
                            .append(PyTuple::new(py, [name.as_bytes(), val.as_bytes()]).unwrap());
                    }

                    let _ = scope.set_item("headers", scope_headers);

                    let _ = scope.set_item("client", "");
                    let _ = scope.set_item("server", "");

                    let clone_body = request_data.body.clone();
                    let receive_callback = move |args: &Bound<'_, PyTuple>,
                                                 _kwargs: Option<&Bound<'_, PyDict>>|
                          -> PyResult<Py<PyAny>> {
                        Python::with_gil(|py| {
                            dbg!(args);
                            dbg!(_kwargs);

                            let callback_body = clone_body.clone();
                            let body = PyBytes::new(py, &callback_body);

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

                    // let tx_for_send = Arc::clone(&data.callback);

                    let clone = asgi_sender.clone();
                    let send_callback = move |args: &Bound<'_, PyTuple>,
                                              _kwargs: Option<&Bound<'_, PyDict>>|
                          -> PyResult<Py<PyAny>> {
                        Python::with_gil(|py| {
                            let data = args.get_item(0).unwrap();
                            let data_type_result = data.get_item("type").unwrap();
                            let data_type = data_type_result.extract::<String>().unwrap();

                            let data_type_ref = data_type.as_str();
                            dbg!(&data_type_ref);

                            match data_type_ref {
                                "http.response.start" => {
                                    let start = HttpResponseStart::new(
                                        &data_type,
                                        data.get_item("status").unwrap().extract::<u16>().unwrap(),
                                    );

                                    clone.send(ASGIMessages::HttpResponseStart(start));
                                    // request_data
                                    // j
                                    //     .callback
                                    //     .send(ASGIMessages::HttpResponseStart(start))
                                    //     .unwrap();
                                }
                                "http.response.body" => {
                                    let body_bytes = data.get_item("body").unwrap();
                                    let body_vec = body_bytes.extract::<Vec<u8>>();

                                    let body = HttpResponseBody::new(body_vec.unwrap());
                                    clone.send(ASGIMessages::HttpResponseBody(body)).unwrap();

                                    // request_data
                                    //     .callback
                                    //     .send(ASGIMessages::HttpResponseBody(body))
                                    //     .unwrap();
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

                    let receive =
                        PyCFunction::new_closure(py, None, None, receive_callback).unwrap();
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

                    let asgi_app = app_module
                        .getattr(PyString::new(py, &asgi_attr_arc))
                        .unwrap();

                    // Create the coroutine to call the FastAPI app
                    let coroutine = asgi_app.call1(asgi_args).unwrap();

                    // Run the coroutine until it completes
                    event_loop
                        .call_method1("run_until_complete", (coroutine,))
                        .unwrap();

                    event_loop.call_method0("close").unwrap();
                }
            });
        });

        Ok(tx)
    }
}

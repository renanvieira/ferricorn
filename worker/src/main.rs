use args::Arguments;
use clap::Parser;
use crossbeam_channel::unbounded;
use messages::types::{ASGIMessages, ParsedRequest};
use py_process::PythonProcess;
use std::{fs::File, process::exit, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, Interest},
    net::UnixListener,
    signal::{unix::signal, unix::SignalKind},
};

pub mod args;
pub mod py_process;

struct Connection {
    pub id: u32,
    pub request: ParsedRequest,
}

#[tokio::main]
async fn main() {
    let cli = Arguments::parse();

    let mut signal_terminate = signal(SignalKind::terminate()).unwrap();
    let mut signal_interrupt = signal(SignalKind::interrupt()).unwrap();

    tokio::select! {
        _ = signal_terminate.recv() => {
            if  cli.sock.exists() {
                std::fs::remove_file(cli.sock).unwrap();
            }
            exit(0)
        },
        _ = signal_interrupt.recv() => {
            if  cli.sock.exists() {
                std::fs::remove_file(cli.sock).unwrap();
            }
            exit(0)
        },
        _ = run_worker(&cli) => { println!("worker finihsed")},
    };
}

async fn run_worker(cli: &Arguments) {
    println!("listening to : {:?}", &cli.sock);
    let listener = UnixListener::bind(cli.sock.clone()).unwrap();
    let (tx_request, rx_request) = unbounded::<Connection>();
    let (tx_response, rx_response) = unbounded::<ASGIMessages>();

    let (module, asgi_attr) = cli.module.split_once(":").unwrap();

    let python_tx =
        PythonProcess::start(module.to_owned(), asgi_attr.to_owned(), tx_response).unwrap();

    let python_tx = Arc::new(python_tx);

    let mut conn_id = 0; // Background task to handle Python communication

    tokio::spawn(async move {
        while let Ok(conn) = rx_request.recv() {
            if let Err(e) = python_tx.send(conn.request) {
                eprintln!("Failed to send to Python: {}", e);
                break;
            }
        }
    });

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let tx_req = tx_request.clone();
                let rx_resp = rx_response.clone();
                let current_id = conn_id;
                conn_id += 1;

                tokio::spawn(async move {
                    handle_connection(stream, tx_req, rx_resp, current_id).await
                });
            }
            Err(err) => eprintln!("Failed to accept connection: {}", err),
        }
    }
}

async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    tx_request: crossbeam_channel::Sender<Connection>,
    rx_response: crossbeam_channel::Receiver<ASGIMessages>,
    conn_id: u32,
) {
    loop {
        let mut len_bytes = [0u8; 4];
        match stream.read_exact(&mut len_bytes).await {
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // Clean exit - client closed connection
                println!("Client disconnected (connection {})", conn_id);
                break;
            }
            Err(e) => {
                eprintln!("Error reading from client (connection {}): {}", conn_id, e);
                break;
            }
        }

        let len = u32::from_be_bytes(len_bytes) as usize;
        let mut buffer = vec![0u8; len];

        match stream.read_exact(&mut buffer).await {
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                println!("Client disconnected mid-message (connection {})", conn_id);
                break;
            }
            Err(e) => {
                eprintln!("Error reading message body (connection {}): {}", conn_id, e);
                break;
            }
        }

        let request: ParsedRequest = match bincode::deserialize(&buffer) {
            Ok(req) => req,
            Err(e) => {
                eprintln!(
                    "Failed to deserialize request (connection {}): {}",
                    conn_id, e
                );
                break;
            }
        };

        // Send request to python process
        if let Err(e) = tx_request.send(Connection {
            id: conn_id,
            request,
        }) {
            eprintln!(
                "Failed to send request to Python (connection {}): {}",
                conn_id, e
            );
            break;
        }

        for i in 0..2 {
            match rx_response.recv() {
                Ok(response) => {
                    let payload = bincode::serialize(&response).unwrap();
                    let payload_len = payload.len() as u32;

                    // Try to send both length and payload
                    let send_result = async {
                        stream.write_all(&payload_len.to_be_bytes()).await?;
                        stream.write_all(&payload).await?;
                        stream.flush().await?;
                        Ok::<_, std::io::Error>(())
                    }
                    .await;

                    if let Err(e) = send_result {
                        if e.kind() == std::io::ErrorKind::BrokenPipe {
                            println!(
                                "Client disconnected while sending response (connection {})",
                                conn_id
                            );
                        } else {
                            eprintln!("Error sending response (connection {}): {}", conn_id, e);
                        }
                        break;
                    }

                    match (i, &response) {
                        (0, ASGIMessages::HttpResponseStart(_)) => continue,
                        (1, ASGIMessages::HttpResponseBody(_)) => (),
                        _ => {
                            eprintln!("Unexpected message order (connection {})", conn_id);
                            break;
                        }
                    }
                }
                Err(e) => {
                    eprintln!(
                        "Error receiving response from Python (connection {}): {}",
                        conn_id, e
                    );
                    break;
                }
            }
        }
    }

    println!("Connection {} closed", conn_id);
}

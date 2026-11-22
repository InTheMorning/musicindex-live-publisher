//! A small local HTTP server for route service tests.

use anyhow::{Context, Result};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// One response from a test HTTP server.
#[derive(Debug, Clone)]
pub enum Reply {
    /// An HTTP response with a status and body.
    Answer { status: u16, body: String },
    /// Wait, then close the connection without a response.
    Hang(Duration),
}

/// A local HTTP server that records request targets.
pub struct TestServer {
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
    stopped: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl TestServer {
    /// Starts a server on an available loopback port.
    pub fn start(handler: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").context("bind test server")?;
        let address = listener.local_addr().context("read test server address")?;
        listener
            .set_nonblocking(true)
            .context("set nonblocking listener")?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server_requests = Arc::clone(&requests);
        let handler = Arc::new(handler);
        let stopped = Arc::new(AtomicBool::new(false));
        let server_stopped = Arc::clone(&stopped);
        let thread = thread::spawn(move || {
            while !server_stopped.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let handler = Arc::clone(&handler);
                        let requests = Arc::clone(&server_requests);
                        thread::spawn(move || handle(stream, handler.as_ref(), &requests));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            base_url: format!("http://{address}"),
            requests,
            stopped,
            thread: Some(thread),
        })
    }

    /// Returns the server endpoint.
    pub fn base_url(&self) -> String {
        self.base_url.clone()
    }

    /// Returns request targets in arrival order.
    pub fn requests(&self) -> Vec<String> {
        self.requests
            .lock()
            .map(|items| items.clone())
            .unwrap_or_default()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.base_url.trim_start_matches("http://"));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn handle(mut stream: TcpStream, handler: &impl Fn(&str) -> Reply, requests: &Mutex<Vec<String>>) {
    let mut buffer = [0_u8; 8192];
    let Ok(size) = stream.read(&mut buffer) else {
        return;
    };
    let request = String::from_utf8_lossy(&buffer[..size]);
    let Some(target) = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
    else {
        return;
    };
    if let Ok(mut items) = requests.lock() {
        items.push(target.to_owned());
    }
    match handler(target) {
        Reply::Hang(duration) => thread::sleep(duration),
        Reply::Answer { status, body } => {
            let reason = match status {
                200 => "OK",
                404 => "Not Found",
                500 => "Internal Server Error",
                _ => "Test",
            };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/json\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    }
}

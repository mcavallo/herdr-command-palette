//! A fake Herdr socket server for tests: answers scripted JSON responses on
//! a Unix socket in a temp dir and records the requests it received.
//!
//! Include from an integration test with
//! `#[path = "support/fake_herdr_server.rs"] mod fake_herdr_server;`
//! (unit tests in `src/adapters/` use `#[path = "../../tests/support/fake_herdr_server.rs"]`).
//!
//! ```ignore
//! let herdr = FakeHerdrServer::scripted([Reply::ok(json!({"actions": []}))]);
//! // point the code under test at herdr.socket_path(), e.g. HERDR_SOCKET_PATH
//! assert_eq!(herdr.methods(), ["plugin.action.list"]);
//! ```
//!
//! Like Herdr, it reads one request line per connection, writes one answer
//! line and closes the connection.

// Reason: each test crate that includes this file uses only part of it.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;

/// What the fake answers to one request.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// `{"id", "result": <value>}`.
    Result(Value),
    /// `{"id", "error": {"code", "message"}}`.
    Error {
        /// Error code, e.g. `ui_busy`.
        code: String,
        /// Error message.
        message: String,
    },
    /// This exact text, sent as is (no newline added): for malformed answers.
    Raw(String),
    /// Close the connection without answering.
    Close,
    /// Say nothing for this long, then close (a hung Herdr).
    Silent(Duration),
}

impl Reply {
    /// A successful answer with this `result`.
    pub fn ok(result: Value) -> Self {
        Self::Result(result)
    }

    /// An error answer.
    pub fn error(code: &str, message: &str) -> Self {
        Self::Error {
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }

    /// A raw answer, sent byte for byte.
    pub fn raw(text: &str) -> Self {
        Self::Raw(text.to_owned())
    }

    /// The bytes to write for a request with id `id`.
    fn to_bytes(&self, id: &Value) -> Vec<u8> {
        let body = match self {
            Self::Result(result) => json!({"id": id, "result": result}),
            Self::Error { code, message } => {
                json!({"id": id, "error": {"code": code, "message": message}})
            }
            Self::Raw(text) => return text.clone().into_bytes(),
            Self::Close | Self::Silent(_) => return Vec::new(),
        };
        format!("{body}\n").into_bytes()
    }
}

/// A Herdr stand-in listening on `<tempdir>/herdr.sock` until dropped.
pub struct FakeHerdrServer {
    socket_path: PathBuf,
    requests: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    // Keeps the socket's directory alive; removed on drop.
    _dir: TempDir,
}

impl FakeHerdrServer {
    /// Starts a server that answers each request with `handler(request)`.
    ///
    /// The handler gets the whole envelope (`{"id", "method", "params"}`);
    /// a request line that isn't JSON is passed as a JSON string.
    pub fn start(handler: impl FnMut(&Value) -> Reply + Send + 'static) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let socket_path = dir.path().join("herdr.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind fake herdr socket");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            // `Arc::clone` gives the server thread its own handle to the
            // shared state; the thread must own everything it touches.
            let (requests, stop) = (Arc::clone(&requests), Arc::clone(&stop));
            std::thread::spawn(move || serve(&listener, handler, &requests, &stop))
        };
        Self {
            socket_path,
            requests,
            stop,
            thread: Some(thread),
            _dir: dir,
        }
    }

    /// Starts a server that answers requests with `replies`, in order. Once
    /// they run out it answers `{"error": {"code": "fake_exhausted"}}`.
    pub fn scripted(replies: impl IntoIterator<Item = Reply>) -> Self {
        let mut replies: VecDeque<Reply> = replies.into_iter().collect();
        Self::start(move |_| {
            replies
                .pop_front()
                .unwrap_or_else(|| Reply::error("fake_exhausted", "no scripted reply left"))
        })
    }

    /// Starts a server that answers by method name, the same reply every
    /// time; other methods get `{"error": {"code": "unknown_method"}}`.
    pub fn by_method<'a>(replies: impl IntoIterator<Item = (&'a str, Reply)>) -> Self {
        let replies: Vec<(String, Reply)> = replies
            .into_iter()
            .map(|(method, reply)| (method.to_owned(), reply))
            .collect();
        Self::start(move |request| {
            let method = request["method"].as_str().unwrap_or_default();
            match replies.iter().find(|(name, _)| name == method) {
                Some((_, reply)) => reply.clone(),
                None => Reply::error("unknown_method", method),
            }
        })
    }

    /// Where the server listens (use as `HERDR_SOCKET_PATH`).
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Every request received so far, as parsed envelopes.
    pub fn requests(&self) -> Vec<Value> {
        self.requests.lock().expect("requests lock").clone()
    }

    /// The `method` of every request received so far.
    pub fn methods(&self) -> Vec<String> {
        self.requests()
            .iter()
            .map(|request| request["method"].as_str().unwrap_or_default().to_owned())
            .collect()
    }
}

impl Drop for FakeHerdrServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // `accept` blocks: a throwaway connection wakes it up to see `stop`.
        let _ = UnixStream::connect(&self.socket_path);
        // A handler that panicked would otherwise only show as refused
        // connections; a test already panicking keeps its own message.
        if let Some(thread) = self.thread.take()
            && let Err(panic) = thread.join()
            && !thread::panicking()
        {
            std::panic::resume_unwind(panic);
        }
    }
}

/// How long the server waits for a client's request line.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Accept loop: one request and one reply per connection.
fn serve(
    listener: &UnixListener,
    mut handler: impl FnMut(&Value) -> Reply,
    requests: &Mutex<Vec<Value>>,
    stop: &AtomicBool,
) {
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let Ok(mut stream) = stream else { continue };
        // A client that never ends its line must not stall the loop (and
        // the join on drop) forever.
        let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
        let mut line = String::new();
        if BufReader::new(&stream).read_line(&mut line).is_err() {
            continue;
        }
        let request = serde_json::from_str(line.trim_end()).unwrap_or(Value::String(line));
        let reply = handler(&request);
        // Recorded before answering, so a client that got its answer sees it.
        requests
            .lock()
            .expect("requests lock")
            .push(request.clone());
        if let Reply::Silent(wait) = reply {
            thread::sleep(wait);
            continue;
        }
        // The client may already be gone; nothing to do about it here.
        let _ = stream.write_all(&reply.to_bytes(&request["id"]));
    }
}

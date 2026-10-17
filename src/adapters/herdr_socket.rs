//! [`HerdrApi`] over Herdr's Unix socket: one JSON request per connection,
//! newline terminated; retries while Herdr answers `ui_busy`, per its
//! [`RetryPolicy`]. A Herdr that doesn't answer within [`TIMEOUT`] is a
//! transport error, so a hung server can't hang the palette with it.
//!
//! The wire format (Herdr `src/api/`, v0.9.3): the client writes
//! `{"id", "method", "params"}` and a newline; Herdr answers one line,
//! `{"id", "result"}` or `{"id", "error": {"code", "message"}}`, and closes
//! the connection.

use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::domain::herdr::Request;
use crate::domain::retry::RetryPolicy;
use crate::ports::{HerdrApi, HerdrError};

/// How long one request may take to send and to answer. Herdr answers in
/// milliseconds, and answers `ui_busy` at once when it can't act yet.
pub const TIMEOUT: Duration = Duration::from_secs(5);

/// Talks to the Herdr server listening on `socket_path`.
///
/// ```no_run
/// use herdr_command_palette::adapters::herdr_socket::UnixSocketHerdr;
/// use herdr_command_palette::ports::HerdrApi;
///
/// let herdr = UnixSocketHerdr::new("/home/me/.config/herdr/herdr.sock");
/// let snapshot = herdr.snapshot()?;
/// # Ok::<(), herdr_command_palette::ports::HerdrError>(())
/// ```
#[derive(Debug, Clone)]
pub struct UnixSocketHerdr {
    socket_path: PathBuf,
    retry: RetryPolicy,
    timeout: Duration,
    // A plain function pointer (not a closure) keeps the struct `Clone` and
    // `Debug`; tests swap in one that records instead of sleeping.
    sleep: fn(Duration),
}

impl UnixSocketHerdr {
    /// A client for the socket at `socket_path` (`PluginEnv::socket_path`)
    /// with the default [`RetryPolicy`] and [`TIMEOUT`].
    #[must_use]
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
            retry: RetryPolicy::default(),
            timeout: TIMEOUT,
            sleep: std::thread::sleep,
        }
    }

    /// The same client with another retry policy.
    #[must_use]
    pub fn with_retry(self, retry: RetryPolicy) -> Self {
        Self { retry, ..self }
    }

    /// The same client waiting at most `timeout` per send and per answer
    /// (for tests).
    #[must_use]
    pub fn with_timeout(self, timeout: Duration) -> Self {
        Self { timeout, ..self }
    }

    /// The same client pausing with `sleep` between retries (for tests).
    #[must_use]
    pub fn with_sleep(self, sleep: fn(Duration)) -> Self {
        Self { sleep, ..self }
    }

    /// One request/response round trip, no retries.
    fn call_once(&self, request: &Request) -> Result<Value, HerdrError> {
        let line = self.exchange(&encode(request))?;
        decode(&line)
    }

    /// Writes `line` and reads the answer line, each within the timeout.
    fn exchange(&self, line: &str) -> Result<String, HerdrError> {
        let path = self.socket_path.display();
        let transport = |error: std::io::Error| match error.kind() {
            // A timed-out read or write: `WouldBlock` on Linux, `TimedOut`
            // elsewhere.
            ErrorKind::WouldBlock | ErrorKind::TimedOut => HerdrError::Transport(format!(
                "{path}: no answer within {} ms",
                self.timeout.as_millis()
            )),
            _ => HerdrError::Transport(format!("{path}: {error}")),
        };
        let mut stream = UnixStream::connect(&self.socket_path).map_err(transport)?;
        stream
            .set_read_timeout(Some(self.timeout))
            .and_then(|()| stream.set_write_timeout(Some(self.timeout)))
            .map_err(transport)?;
        stream.write_all(line.as_bytes()).map_err(transport)?;
        let mut answer = String::new();
        // Herdr closes the connection after answering; `read_line` also
        // stops at EOF, so a missing final newline is fine.
        BufReader::new(stream)
            .read_line(&mut answer)
            .map_err(transport)?;
        if answer.trim().is_empty() {
            return Err(transport(std::io::Error::new(
                ErrorKind::UnexpectedEof,
                "connection closed without an answer",
            )));
        }
        Ok(answer)
    }
}

impl HerdrApi for UnixSocketHerdr {
    fn call(&self, request: &Request) -> Result<Value, HerdrError> {
        let mut attempt = 1;
        loop {
            match self.call_once(request) {
                Err(error) if error.is_ui_busy() && attempt < self.retry.attempts => {
                    (self.sleep)(self.retry.delay);
                    attempt += 1;
                }
                outcome => return outcome,
            }
        }
    }
}

/// The answer envelope; `id` is ignored (one request per connection).
#[derive(Debug, Deserialize)]
struct Answer {
    result: Option<Value>,
    error: Option<ErrorBody>,
}

/// `{"code", "message"}` of an error answer; both optional.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ErrorBody {
    code: String,
    message: String,
}

/// The request as one JSON line; the id identifies the palette.
fn encode(request: &Request) -> String {
    let envelope = json!({
        "id": format!("command-palette:{}", request.method),
        "method": request.method,
        "params": request.params,
    });
    // `Value`'s `Display` is compact JSON and can't fail.
    format!("{envelope}\n")
}

/// The `result` of an answer line (`{}` when absent), or its error.
fn decode(line: &str) -> Result<Value, HerdrError> {
    let answer: Answer =
        serde_json::from_str(line).map_err(|error| HerdrError::Decode(error.to_string()))?;
    match answer.error {
        Some(ErrorBody { code, message }) => Err(HerdrError::Api { code, message }),
        None => Ok(answer.result.unwrap_or_else(|| Value::Object(Map::new()))),
    }
}

#[cfg(test)]
#[path = "../../tests/support/fake_herdr_server.rs"]
mod fake_herdr_server;

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use serde_json::json;

    use super::fake_herdr_server::{FakeHerdrServer, Reply};
    use super::*;

    thread_local! {
        // Each test runs on its own thread, so this log is per test.
        static SLEEPS: RefCell<Vec<Duration>> = const { RefCell::new(Vec::new()) };
    }

    fn record_sleep(delay: Duration) {
        SLEEPS.with(|sleeps| sleeps.borrow_mut().push(delay));
    }

    fn recorded_sleeps() -> Vec<Duration> {
        SLEEPS.with(|sleeps| sleeps.borrow().clone())
    }

    fn client(herdr: &FakeHerdrServer) -> UnixSocketHerdr {
        UnixSocketHerdr::new(herdr.socket_path()).with_sleep(record_sleep)
    }

    fn busy() -> Reply {
        Reply::error("ui_busy", "popup closing")
    }

    #[test]
    fn sends_the_envelope_and_returns_the_result() {
        let herdr = FakeHerdrServer::scripted([Reply::ok(json!({"type": "ok", "n": 1}))]);
        let request = Request::new("pane.focus", json!({"pane_id": "w1:p2"}));

        let result = client(&herdr).call(&request).unwrap();

        assert_eq!(result, json!({"type": "ok", "n": 1}));
        assert_eq!(
            herdr.requests(),
            [json!({
                "id": "command-palette:pane.focus",
                "method": "pane.focus",
                "params": {"pane_id": "w1:p2"}
            })]
        );
    }

    #[test]
    fn missing_result_is_an_empty_object() {
        let herdr = FakeHerdrServer::scripted([Reply::raw("{\"id\": \"x\"}")]);

        let result = client(&herdr).call(&Request::without_params("ping"));

        assert_eq!(result, Ok(json!({})));
    }

    #[test]
    fn error_body_becomes_an_api_error() {
        let herdr = FakeHerdrServer::scripted([Reply::error("pane_not_found", "no pane w9")]);

        let error = client(&herdr)
            .call(&Request::without_params("pane.focus"))
            .unwrap_err();

        assert_eq!(
            error,
            HerdrError::Api {
                code: "pane_not_found".into(),
                message: "no pane w9".into()
            }
        );
    }

    #[test]
    fn error_body_without_fields_is_still_an_error() {
        let herdr = FakeHerdrServer::scripted([Reply::raw("{\"error\": {}}\n")]);

        let result = client(&herdr).call(&Request::without_params("ping"));

        assert_eq!(
            result,
            Err(HerdrError::Api {
                code: String::new(),
                message: String::new()
            })
        );
    }

    #[test]
    fn retries_while_ui_busy_then_succeeds() {
        let herdr = FakeHerdrServer::scripted([busy(), busy(), Reply::ok(json!({"done": true}))]);

        let result = client(&herdr).call(&Request::without_params("tab.create"));

        assert_eq!(result, Ok(json!({"done": true})));
        assert_eq!(herdr.methods(), ["tab.create"; 3]);
        assert_eq!(recorded_sleeps(), [Duration::from_millis(50); 2]);
    }

    #[test]
    fn gives_up_after_the_last_attempt() {
        let herdr = FakeHerdrServer::scripted(vec![busy(); 5]);
        let retry = RetryPolicy {
            attempts: 3,
            delay: Duration::from_millis(7),
        };

        let error = client(&herdr)
            .with_retry(retry)
            .call(&Request::without_params("tab.create"))
            .unwrap_err();

        assert!(error.is_ui_busy());
        assert_eq!(herdr.requests().len(), 3);
        assert_eq!(recorded_sleeps(), [Duration::from_millis(7); 2]);
    }

    #[test]
    fn other_errors_are_not_retried() {
        let herdr = FakeHerdrServer::scripted([Reply::error("invalid_request", "bad"), busy()]);

        let error = client(&herdr)
            .call(&Request::without_params("x"))
            .unwrap_err();

        assert_eq!(error.code(), Some("invalid_request"));
        assert_eq!(herdr.requests().len(), 1);
        assert_eq!(recorded_sleeps(), Vec::<Duration>::new());
    }

    #[test]
    fn no_retry_policy_tries_once() {
        let herdr = FakeHerdrServer::scripted([busy(), Reply::ok(json!({}))]);

        let result = client(&herdr)
            .with_retry(RetryPolicy::NONE)
            .call(&Request::without_params("x"));

        assert!(result.unwrap_err().is_ui_busy());
        assert_eq!(herdr.requests().len(), 1);
    }

    #[test]
    fn default_policy_is_forty_times_fifty_ms() {
        assert_eq!(
            RetryPolicy::default(),
            RetryPolicy {
                attempts: 40,
                delay: Duration::from_millis(50)
            }
        );
    }

    #[test]
    fn default_sleep_really_pauses() {
        let herdr = FakeHerdrServer::scripted([busy(), Reply::ok(json!({}))]);
        let retry = RetryPolicy {
            attempts: 2,
            delay: Duration::from_millis(1),
        };

        let result = UnixSocketHerdr::new(herdr.socket_path())
            .with_retry(retry)
            .call(&Request::without_params("x"));

        assert_eq!(result, Ok(json!({})));
    }

    #[test]
    fn missing_socket_is_a_transport_error_naming_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("herdr.sock");

        let error = UnixSocketHerdr::new(&path)
            .call(&Request::without_params("ping"))
            .unwrap_err();

        let prefix = format!("{}: ", path.display());
        assert!(
            matches!(&error, HerdrError::Transport(message) if message.starts_with(&prefix)),
            "{error:?}"
        );
    }

    #[test]
    fn hang_up_without_answer_is_a_transport_error() {
        let herdr = FakeHerdrServer::scripted([Reply::Close]);

        let client = client(&herdr);

        let result = client.call(&Request::without_params("ping"));

        assert_eq!(
            result,
            Err(HerdrError::Transport(format!(
                "{}: connection closed without an answer",
                client.socket_path.display()
            )))
        );
    }

    #[test]
    fn a_silent_herdr_is_a_transport_error_after_the_timeout() {
        let herdr = FakeHerdrServer::scripted([Reply::Silent(Duration::from_millis(300))]);
        let client = client(&herdr).with_timeout(Duration::from_millis(50));

        let started = std::time::Instant::now();
        let result = client.call(&Request::without_params("ping"));

        assert_eq!(
            result,
            Err(HerdrError::Transport(format!(
                "{}: no answer within 50 ms",
                client.socket_path.display()
            )))
        );
        // Gave up at the timeout, before the server's silence ended.
        let elapsed = started.elapsed();
        assert!(elapsed < Duration::from_millis(300), "{elapsed:?}");
    }

    #[test]
    fn default_timeout_is_five_seconds() {
        assert_eq!(UnixSocketHerdr::new("/x").timeout, Duration::from_secs(5));
    }

    #[test]
    fn invalid_json_is_a_decode_error() {
        let herdr = FakeHerdrServer::scripted([Reply::raw("not json\n")]);

        let result = client(&herdr).call(&Request::without_params("ping"));

        assert!(matches!(result, Err(HerdrError::Decode(_))), "{result:?}");
    }

    #[test]
    fn notify_shows_a_toast() {
        let herdr = FakeHerdrServer::scripted([Reply::ok(json!({"type": "ok"}))]);

        client(&herdr).notify("pane gone").unwrap();

        assert_eq!(
            herdr.requests()[0]["params"],
            json!({"title": "command-palette", "body": "pane gone"})
        );
        assert_eq!(herdr.methods(), ["notification.show"]);
    }

    #[test]
    fn typed_helpers_work_over_the_socket() {
        let herdr = FakeHerdrServer::by_method([
            (
                "session.snapshot",
                Reply::ok(json!({"type": "session_snapshot", "snapshot": {
                    "workspaces": [{"workspace_id": "w1", "label": "alpha"}],
                    "tabs": [], "panes": []
                }})),
            ),
            (
                "plugin.action.list",
                Reply::ok(json!({"actions": [
                    {"plugin_id": "demo", "action_id": "hi", "title": "Hi"}
                ]})),
            ),
        ]);
        let api = client(&herdr);

        let snapshot = api.snapshot().unwrap();
        let actions = api.plugin_actions().unwrap();
        let unknown = api.call(&Request::without_params("nope")).unwrap_err();

        assert_eq!(snapshot.workspaces[0].label, "alpha");
        assert_eq!(actions[0].qualified_id(), "demo.hi");
        assert_eq!(unknown.code(), Some("unknown_method"));
        assert_eq!(
            herdr.methods(),
            ["session.snapshot", "plugin.action.list", "nope"]
        );
    }

    #[test]
    fn fake_records_non_json_requests_as_strings() {
        let herdr = FakeHerdrServer::scripted([]);

        let answer = client(&herdr).exchange("hello\n").unwrap();

        assert_eq!(herdr.requests(), [json!("hello\n")]);
        assert!(answer.contains("fake_exhausted"), "{answer}");
    }
}

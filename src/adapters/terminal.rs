//! [`Terminal`] for the popup: its width, reading one key and asking its
//! light/dark appearance, through `stty` on `/dev/tty` (POSIX, no
//! `unsafe` ioctl).
//!
//! All are best effort: without a terminal (tests, pipes) the width and
//! the appearance are unknown and reading a key returns at once, with no
//! key. Standard input is never used, so a piped stdin changes nothing.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::domain::appearance::{self, Appearance};
use crate::ports::Terminal;

/// How long one read waits for a reply byte, in tenths of a second
/// (`stty time`). Herdr answers from its own state at once; this only
/// bounds a terminal that doesn't answer.
const REPLY_WAIT_TENTHS: &str = "1";

/// The most reply bytes read: three short replies fit many times over.
const MAX_REPLY_BYTES: usize = 1024;

/// The controlling terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tty {
    device: PathBuf,
    stty: PathBuf,
}

impl Tty {
    /// `/dev/tty`, configured with `stty` from `PATH`.
    #[must_use]
    pub fn new() -> Self {
        Self::with_paths("/dev/tty", "stty")
    }

    /// Another device and `stty` (tests use a file and a stub).
    #[must_use]
    pub fn with_paths(device: impl Into<PathBuf>, stty: impl Into<PathBuf>) -> Self {
        Self {
            device: device.into(),
            stty: stty.into(),
        }
    }

    /// Runs `stty args` on the device; its trimmed output on success.
    fn stty(&self, args: &[&str]) -> Option<String> {
        let device = File::open(&self.device).ok()?;
        let output = Command::new(&self.stty)
            .args(args)
            .stdin(device)
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())?;
        Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
}

impl Default for Tty {
    fn default() -> Self {
        Self::new()
    }
}

impl Terminal for Tty {
    fn columns(&self) -> Option<usize> {
        // `stty size` prints "rows columns".
        self.stty(&["size"])?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    }

    fn read_key(&self) -> Option<u8> {
        let mut device = File::open(&self.device).ok()?;
        let saved = self.stty(&["-g"]);
        // One key, no Enter, no echo (bash `read -rsn1`). `min 1 time 0`
        // because leaving canonical mode keeps stale values there.
        self.stty(&["-icanon", "-echo", "min", "1", "time", "0"]);
        let mut key = [0_u8; 1];
        // Any outcome ends the wait: a key, EOF or an error.
        let read = device.read(&mut key);
        if let Some(saved) = saved {
            self.stty(&[&saved]);
        }
        let [byte] = key;
        match read {
            Ok(1) => Some(byte),
            _ => None,
        }
    }

    fn appearance(&self) -> Option<Appearance> {
        let mut device = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.device)
            .ok()?;
        // Not a terminal (or no `stty`): ask nothing, read nothing.
        let saved = self.stty(&["-g"])?;
        // Replies arrive without Enter and aren't echoed; with `min 0` a
        // read returns empty after `time` without input, so a silent
        // terminal can't hang the palette.
        let raw = self.stty(&["-icanon", "-echo", "min", "0", "time", REPLY_WAIT_TENTHS]);
        let replies = if raw.is_some() {
            exchange(&mut device)
        } else {
            Vec::new()
        };
        self.stty(&[&saved]);
        appearance::from_replies(&replies)
    }
}

/// Writes [`appearance::QUERY`] to `stream` and collects the replies until
/// they are [complete](appearance::is_complete), a read comes back empty
/// (timeout or end of input) or fails, or [`MAX_REPLY_BYTES`] arrived.
fn exchange(stream: &mut (impl Read + Write)) -> Vec<u8> {
    let mut replies = Vec::new();
    if stream
        .write_all(appearance::QUERY)
        .and_then(|()| stream.flush())
        .is_err()
    {
        return replies;
    }
    let mut chunk = [0_u8; 256];
    while replies.len() < MAX_REPLY_BYTES && !appearance::is_complete(&replies) {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => replies.extend(chunk.iter().take(read)),
        }
    }
    replies
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;
    use std::path::Path;

    use crate::adapters::scripts;

    use super::*;

    /// A fake `stty` logging its arguments to `log`.
    fn stty(dir: &Path, log: &Path) -> PathBuf {
        let body = format!(
            "echo \"$*\" >> '{}'\ncase \"$1\" in\n  size) echo '24 100' ;;\n  -g) echo 'saved:state' ;;\nesac",
            log.display()
        );
        scripts::script(dir, "stty", &body)
    }

    #[test]
    fn measures_columns_with_stty_size() {
        let dir = tempfile::tempdir().unwrap();
        let device = dir.path().join("tty");
        fs::write(&device, "").unwrap();
        let log = dir.path().join("log");
        let tty = Tty::with_paths(&device, stty(dir.path(), &log));

        assert_eq!(tty.columns(), Some(100));
    }

    #[test]
    fn waits_for_one_key_and_restores_the_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let device = dir.path().join("tty");
        fs::write(&device, "xy").unwrap();
        let log = dir.path().join("log");
        let tty = Tty::with_paths(&device, stty(dir.path(), &log));

        let key = tty.read_key();

        assert_eq!(key, Some(b'x'));
        assert_eq!(
            fs::read_to_string(&log).unwrap(),
            "-g\n-icanon -echo min 1 time 0\nsaved:state\n"
        );
    }

    #[test]
    fn reads_no_key_at_end_of_input() {
        let dir = tempfile::tempdir().unwrap();
        let device = dir.path().join("tty");
        fs::write(&device, "").unwrap();
        let log = dir.path().join("log");
        let tty = Tty::with_paths(&device, stty(dir.path(), &log));

        assert_eq!(tty.read_key(), None);
    }

    #[test]
    fn without_a_terminal_device_nothing_is_measured_or_awaited() {
        let missing = Tty::with_paths("/nonexistent/tty", "stty");

        missing.wait_for_key();

        assert_eq!(missing.columns(), None);
    }

    #[test]
    fn a_plain_file_is_not_measured_or_awaited() {
        let dir = tempfile::tempdir().unwrap();
        let device = dir.path().join("tty");
        fs::write(&device, "").unwrap();
        // A plain file isn't a terminal: real stty fails, nothing restored.
        let file = Tty::with_paths(&device, "stty");

        file.wait_for_key();

        assert_eq!(file.columns(), None);
    }

    /// A terminal that answers with `replies` and records what it was sent.
    struct FakeStream {
        replies: io::Cursor<Vec<u8>>,
        sent: Vec<u8>,
        fail_writes: bool,
    }

    impl FakeStream {
        fn new(replies: &[u8]) -> Self {
            Self {
                replies: io::Cursor::new(replies.to_vec()),
                sent: Vec::new(),
                fail_writes: false,
            }
        }
    }

    impl Read for FakeStream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.replies.read(buf)
        }
    }

    impl Write for FakeStream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.fail_writes {
                return Err(io::Error::other("gone"));
            }
            self.sent.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// A terminal that answers every read with an error.
    struct BrokenStream;

    impl Read for BrokenStream {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("broken"))
        }
    }

    impl Write for BrokenStream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// A terminal that never stops sending `x`.
    struct ChattyStream;

    impl Read for ChattyStream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            buf.fill(b'x');
            Ok(buf.len())
        }
    }

    impl Write for ChattyStream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn sends_the_query_and_stops_at_the_device_attributes_reply() {
        let mut stream = FakeStream::new(b"\x1b[?997;2n\x1b[?62;22ctyped ahead");

        let replies = exchange(&mut stream);

        assert_eq!(stream.sent, appearance::QUERY);
        assert_eq!(replies, b"\x1b[?997;2n\x1b[?62;22ctyped ahead");
        assert_eq!(appearance::from_replies(&replies), Some(Appearance::Light));
    }

    #[test]
    fn reads_until_the_terminal_goes_quiet() {
        let mut stream = FakeStream::new(b"\x1b]11;rgb:0/0/0\x07");

        let replies = exchange(&mut stream);

        assert_eq!(appearance::from_replies(&replies), Some(Appearance::Dark));
    }

    #[test]
    fn a_query_that_cannot_be_sent_reads_nothing() {
        let mut stream = FakeStream::new(b"\x1b[?997;2n");
        stream.fail_writes = true;

        assert_eq!(exchange(&mut stream), b"");
    }

    #[test]
    fn a_read_error_ends_the_exchange() {
        assert_eq!(exchange(&mut BrokenStream), b"");
    }

    #[test]
    fn a_terminal_that_keeps_talking_is_cut_off() {
        let replies = exchange(&mut ChattyStream);

        assert_eq!(replies.len(), MAX_REPLY_BYTES);
    }

    /// A device file holding `replies` after room for the query, which
    /// shares the file's offset with them.
    fn device_with_replies(dir: &Path, replies: &[u8]) -> PathBuf {
        let device = dir.join("tty");
        let mut contents = vec![b'.'; appearance::QUERY.len()];
        contents.extend_from_slice(replies);
        fs::write(&device, contents).unwrap();
        device
    }

    #[test]
    fn asks_the_appearance_in_raw_mode_and_restores_the_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let device = device_with_replies(dir.path(), b"\x1b[?997;1n\x1b[?62;22c");
        let log = dir.path().join("log");
        let tty = Tty::with_paths(&device, stty(dir.path(), &log));

        assert_eq!(tty.appearance(), Some(Appearance::Dark));

        assert!(fs::read(&device).unwrap().starts_with(appearance::QUERY));
        assert_eq!(
            fs::read_to_string(&log).unwrap(),
            "-g\n-icanon -echo min 0 time 1\nsaved:state\n"
        );
    }

    #[test]
    fn asks_nothing_when_raw_mode_fails() {
        let dir = tempfile::tempdir().unwrap();
        let device = device_with_replies(dir.path(), b"\x1b[?997;1n");
        let log = dir.path().join("log");
        let body = format!(
            "echo \"$*\" >> '{}'\ncase \"$1\" in\n  -g) echo 'saved:state' ;;\n  -icanon) exit 1 ;;\nesac",
            log.display()
        );
        let tty = Tty::with_paths(&device, scripts::script(dir.path(), "stty", &body));

        assert_eq!(tty.appearance(), None);

        assert!(fs::read(&device).unwrap().starts_with(b"."));
        assert!(fs::read_to_string(&log).unwrap().ends_with("saved:state\n"));
    }

    #[test]
    fn without_a_terminal_the_appearance_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let device = device_with_replies(dir.path(), b"\x1b[?997;1n");

        // No device; then a plain file, which real stty rejects.
        assert_eq!(
            Tty::with_paths("/nonexistent/tty", "stty").appearance(),
            None
        );
        assert_eq!(Tty::with_paths(&device, "stty").appearance(), None);

        assert!(fs::read(&device).unwrap().starts_with(b"."));
    }

    #[test]
    fn default_is_new() {
        assert_eq!(Tty::default(), Tty::new());
    }
}

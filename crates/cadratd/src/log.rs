//! The journal: one line per event on standard error (spec daemon §7).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write;
use std::sync::{Mutex, PoisonError};

/// Writes log lines with every device ID and slot identifier replaced by
/// `id-N`, numbered once for the whole process.
pub struct Log {
    out: Mutex<Box<dyn Write + Send>>,
    ids: Mutex<HashMap<String, usize>>,
    journal: bool,
}

impl Log {
    /// `journal`: the stream is the journal's, so lines carry their
    /// priority (spec daemon §7).
    pub fn new(out: Box<dyn Write + Send>, journal: bool) -> Self {
        Self {
            out: Mutex::new(out),
            ids: Mutex::new(HashMap::new()),
            journal,
        }
    }

    /// Writes one line. Logging is best effort.
    pub fn line(&self, text: &str) {
        let text = self.scrub(text);
        let level = if !self.journal {
            ""
        } else if text.starts_with("error:") {
            "<3>"
        } else if text.starts_with("warning:") {
            "<4>"
        } else {
            ""
        };
        let mut out = self.out.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = writeln!(out, "{level}{text}");
        let _ = out.flush();
    }

    /// Replaces each run of exactly 12 lowercase hex digits (a 6-byte ID as
    /// `cadrat` prints it) with its `id-N`.
    fn scrub(&self, text: &str) -> String {
        let mut ids = self.ids.lock().unwrap_or_else(PoisonError::into_inner);
        let bytes = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while i < bytes.len() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
                i += 1;
            }
            if i > start {
                let word = &text[start..i];
                let id = word.len() == 12
                    && word
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
                if id {
                    let next = ids.len() + 1;
                    let n = *ids.entry(word.to_owned()).or_insert(next);
                    let _ = write!(out, "id-{n}");
                } else {
                    out.push_str(word);
                }
            } else {
                let c = text[i..].chars().next().expect("i is inside the text");
                out.push(c);
                i += c.len_utf8();
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_hidden_and_numbered_for_the_process() {
        let log = Log::new(Box::new(std::io::sink()), false);
        assert_eq!(
            log.scrub("mouse c658:0a1b2c3d4e5f → slot id 112233445566"),
            "mouse c658:id-1 → slot id id-2"
        );
        assert_eq!(log.scrub("again 0a1b2c3d4e5f"), "again id-1");
        // Wire reports, ports and longer hex runs stay.
        assert_eq!(
            log.scrub("10 00 05 recv:port-1-4 0a1b2c3d4e5f00 abcdef"),
            "10 00 05 recv:port-1-4 0a1b2c3d4e5f00 abcdef"
        );
    }

    /// A writer the test can read back.
    #[derive(Clone, Default)]
    struct Shared(std::sync::Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn priorities_only_on_the_journal() {
        let lines = [
            "cadratd 0.1.0 starting",
            "warning: W-RAW: raw",
            "error: no bus",
        ];
        for (journal, expected) in [
            (
                true,
                "cadratd 0.1.0 starting\n<4>warning: W-RAW: raw\n<3>error: no bus\n",
            ),
            (
                false,
                "cadratd 0.1.0 starting\nwarning: W-RAW: raw\nerror: no bus\n",
            ),
        ] {
            let out = Shared::default();
            let log = Log::new(Box::new(out.clone()), journal);
            for line in lines {
                log.line(line);
            }
            assert_eq!(
                String::from_utf8(out.0.lock().unwrap().clone()).unwrap(),
                expected
            );
        }
    }
}

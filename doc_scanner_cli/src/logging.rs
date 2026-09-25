use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Where the process's structured log output (design doc §14) goes: stderr by
/// default, or a file when `--log-file` is given. Never a network sink — logs
/// stay local, per AGENTS.md's local-only constraint.
#[derive(Clone)]
pub enum LogSink {
    Stderr,
    File(Arc<Mutex<File>>),
}

impl LogSink {
    pub fn stderr() -> Self {
        LogSink::Stderr
    }

    pub fn file(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(LogSink::File(Arc::new(Mutex::new(file))))
    }
}

pub struct LogSinkWriter(LogSink);

impl Write for LogSinkWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match &self.0 {
            LogSink::Stderr => io::stderr().write(buf),
            LogSink::File(file) => file.lock().expect("log file mutex poisoned").write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match &self.0 {
            LogSink::Stderr => io::stderr().flush(),
            LogSink::File(file) => file.lock().expect("log file mutex poisoned").flush(),
        }
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogSink {
    type Writer = LogSinkWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LogSinkWriter(self.clone())
    }
}

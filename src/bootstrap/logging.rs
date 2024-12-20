//! Setup for the application logging.
//!
//! It redirects the log info to the standard output with the log threshold
//! defined in the configuration.
//!
//! - `Off`
//! - `Error`
//! - `Warn`
//! - `Info`
//! - `Debug`
//! - `Trace`
//!
//! Refer to the [configuration crate documentation](https://docs.rs/torrust-tracker-configuration) to know how to change log settings.
use std::collections::VecDeque;
use std::io;
use std::sync::{Mutex, MutexGuard, Once, OnceLock};

use torrust_tracker_configuration::{Configuration, Threshold};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;

static INIT: Once = Once::new();

/// A global buffer containing the latest lines captured from logs.
#[doc(hidden)]
pub fn captured_logs_buffer() -> &'static Mutex<CircularBuffer> {
    static CAPTURED_LOGS_GLOBAL_BUFFER: OnceLock<Mutex<CircularBuffer>> = OnceLock::new();
    CAPTURED_LOGS_GLOBAL_BUFFER.get_or_init(|| Mutex::new(CircularBuffer::new(1000, 200)))
    // 1000 lines, 200 bytes average
}

/// It redirects the log info to the standard output with the log threshold
/// defined in the configuration.
pub fn setup(cfg: &Configuration) {
    let tracing_level = map_to_tracing_level_filter(&cfg.logging.threshold);

    if tracing_level == LevelFilter::OFF {
        return;
    }

    INIT.call_once(|| {
        tracing_init(tracing_level, &TraceStyle::Default);
    });
}

fn map_to_tracing_level_filter(threshold: &Threshold) -> LevelFilter {
    match threshold {
        Threshold::Off => LevelFilter::OFF,
        Threshold::Error => LevelFilter::ERROR,
        Threshold::Warn => LevelFilter::WARN,
        Threshold::Info => LevelFilter::INFO,
        Threshold::Debug => LevelFilter::DEBUG,
        Threshold::Trace => LevelFilter::TRACE,
    }
}

fn tracing_init(filter: LevelFilter, style: &TraceStyle) {
    let mock_writer = LogCapturer::new(captured_logs_buffer());

    let builder = tracing_subscriber::fmt()
        .with_max_level(filter)
        .with_ansi(true)
        .with_test_writer()
        .with_writer(mock_writer);

    let () = match style {
        TraceStyle::Default => builder.init(),
        TraceStyle::Pretty(display_filename) => builder.pretty().with_file(*display_filename).init(),
        TraceStyle::Compact => builder.compact().init(),
        TraceStyle::Json => builder.json().init(),
    };

    tracing::info!("Logging initialized");
}

#[derive(Debug)]
pub enum TraceStyle {
    Default,
    Pretty(bool),
    Compact,
    Json,
}

impl std::fmt::Display for TraceStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let style = match self {
            TraceStyle::Default => "Default Style",
            TraceStyle::Pretty(path) => match path {
                true => "Pretty Style with File Paths",
                false => "Pretty Style without File Paths",
            },
            TraceStyle::Compact => "Compact Style",
            TraceStyle::Json => "Json Format",
        };

        f.write_str(style)
    }
}

/// It returns true is there is a log line containing all the words passed.
///
/// # Panics
///
/// Will panic if it can't get the lock for the global buffer or convert it into
/// a vec.
#[must_use]
pub fn logs_contains_a_line_with(words: &[&str]) -> bool {
    // code-review: we can search directly in the buffer instead of converting
    // the buffer into a string but that would slow down the tests because
    // cloning should be faster that locking the buffer for searching.
    // Because the buffer is not big.
    let logs = String::from_utf8(captured_logs_buffer().lock().unwrap().as_vec()).unwrap();

    for line in logs.split('\n') {
        if contains_words_in_any_order(line, words) {
            return true;
        }
    }

    false
}

fn contains_words_in_any_order(text: &str, words: &[&str]) -> bool {
    words.iter().all(|&word| text.contains(word))
}

/// A tracing writer which captures the latests logs lines into a buffer.
/// It's used to capture the logs in the tests.
#[derive(Debug)]
pub struct LogCapturer<'a> {
    logs: &'a Mutex<CircularBuffer>,
}

impl<'a> LogCapturer<'a> {
    pub fn new(buf: &'a Mutex<CircularBuffer>) -> Self {
        Self { logs: buf }
    }

    fn buf(&self) -> io::Result<MutexGuard<'a, CircularBuffer>> {
        self.logs.lock().map_err(|_| io::Error::from(io::ErrorKind::Other))
    }
}

impl io::Write for LogCapturer<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        print!("{}", String::from_utf8(buf.to_vec()).unwrap());

        let mut target = self.buf()?;

        target.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.buf()?.flush()
    }
}

impl MakeWriter<'_> for LogCapturer<'_> {
    type Writer = Self;

    fn make_writer(&self) -> Self::Writer {
        LogCapturer::new(self.logs)
    }
}

#[derive(Debug)]
pub struct CircularBuffer {
    max_size: usize,
    buffer: VecDeque<u8>,
}

impl CircularBuffer {
    #[must_use]
    pub fn new(max_lines: usize, average_line_size: usize) -> Self {
        Self {
            max_size: max_lines * average_line_size,
            buffer: VecDeque::with_capacity(max_lines * average_line_size),
        }
    }

    /// # Errors
    ///
    /// Won't return any error.
    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        for &byte in buf {
            if self.buffer.len() == self.max_size {
                // Remove oldest byte to make space
                self.buffer.pop_front();
            }
            self.buffer.push_back(byte);
        }

        Ok(buf.len())
    }

    /// # Errors
    ///
    /// Won't return any error.
    pub fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }

    #[must_use]
    pub fn as_vec(&self) -> Vec<u8> {
        self.buffer.iter().copied().collect()
    }
}

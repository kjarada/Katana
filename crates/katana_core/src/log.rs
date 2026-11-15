use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum LogLevel {
    Trace,
    Debug,
    #[default]
    Info,
    Warning,
    Error,
    Critical,
}

pub const fn to_string(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Trace => "TRACE",
        LogLevel::Debug => "DEBUG",
        LogLevel::Info => "INFO",
        LogLevel::Warning => "WARNING",
        LogLevel::Error => "ERROR",
        LogLevel::Critical => "CRITICAL",
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(to_string(*self))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LogField {
    pub key: String,
    pub value: String,
}

impl LogField {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LogRecord {
    pub time: SystemTime,
    pub level: LogLevel,
    pub category: String,
    pub message: String,
    pub fields: Vec<LogField>,
}

impl Default for LogRecord {
    fn default() -> Self {
        Self {
            time: UNIX_EPOCH,
            level: LogLevel::Info,
            category: String::new(),
            message: String::new(),
            fields: Vec::new(),
        }
    }
}

#[doc = "Gregorian era decomposition from Howard Hinnant's civil_from_days algorithm, relative to the Unix epoch."]
fn format_timestamp(time: SystemTime) -> String {
    let nanos = match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos() as i128,
        Err(error) => -(error.duration().as_nanos() as i128),
    };
    let seconds = nanos / 1_000_000_000;
    let millis = (nanos % 1_000_000_000) / 1_000_000;
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let civil_days = days + 719_468;
    let era = civil_days.div_euclid(146_097);
    let day_of_era = civil_days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let march_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * march_month + 2) / 5 + 1;
    let month = march_month + if march_month < 10 { 3 } else { -9 };
    year += i128::from(month <= 2);
    let hour = day_seconds / 3_600;
    let minute = (day_seconds / 60) % 60;
    let second = day_seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

fn format_field_value(value: &str) -> String {
    if !value.is_empty()
        && !value
            .bytes()
            .any(|byte| matches!(byte, b' ' | b'\t' | b'"' | b'='))
    {
        return value.into();
    }
    let mut quoted = String::from("\"");
    for character in value.chars() {
        if matches!(character, '"' | '\\') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted
}

pub fn format_record(record: &LogRecord) -> String {
    let mut line = format!(
        "{} {} [{}] {}",
        format_timestamp(record.time),
        record.level,
        record.category,
        record.message
    );
    for field in &record.fields {
        line.push(' ');
        line.push_str(&field.key);
        line.push('=');
        line.push_str(&format_field_value(&field.value));
    }
    line
}

pub type Sink = Box<dyn FnMut(&LogRecord) + Send>;

#[derive(Default)]
struct LoggerState {
    sinks: Vec<Sink>,
    minimum_level: LogLevel,
}

#[derive(Default)]
pub struct Logger {
    state: Mutex<LoggerState>,
}

impl Logger {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, LoggerState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub fn add_sink(&self, sink: impl FnMut(&LogRecord) + Send + 'static) {
        self.lock().sinks.push(Box::new(sink));
    }

    pub fn set_minimum_level(&self, level: LogLevel) {
        self.lock().minimum_level = level;
    }

    pub fn minimum_level(&self) -> LogLevel {
        self.lock().minimum_level
    }

    pub fn log(&self, level: LogLevel, category: &str, message: &str, fields: &[LogField]) {
        let mut state = self.lock();
        if level < state.minimum_level || state.sinks.is_empty() {
            return;
        }
        let record = LogRecord {
            time: SystemTime::now(),
            level,
            category: category.into(),
            message: message.into(),
            fields: fields.to_vec(),
        };
        for sink in &mut state.sinks {
            sink(&record);
        }
    }

    pub fn trace(&self, category: &str, message: &str, fields: &[LogField]) {
        self.log(LogLevel::Trace, category, message, fields);
    }

    pub fn debug(&self, category: &str, message: &str, fields: &[LogField]) {
        self.log(LogLevel::Debug, category, message, fields);
    }

    pub fn info(&self, category: &str, message: &str, fields: &[LogField]) {
        self.log(LogLevel::Info, category, message, fields);
    }

    pub fn warning(&self, category: &str, message: &str, fields: &[LogField]) {
        self.log(LogLevel::Warning, category, message, fields);
    }

    pub fn error(&self, category: &str, message: &str, fields: &[LogField]) {
        self.log(LogLevel::Error, category, message, fields);
    }

    pub fn critical(&self, category: &str, message: &str, fields: &[LogField]) {
        self.log(LogLevel::Critical, category, message, fields);
    }
}

pub fn make_stderr_sink() -> Sink {
    Box::new(|record| {
        let _ = writeln!(std::io::stderr().lock(), "{}", format_record(record));
    })
}

pub fn make_file_sink(path: impl Into<String>) -> Sink {
    let path = path.into();
    Box::new(move |record| {
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(file, "{}", format_record(record));
        }
    })
}

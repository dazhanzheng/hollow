use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, Once, OnceLock};

use tracing::field::{Field, Visit};
use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

const BUFFER_CAPACITY: usize = 5000;

static BUFFER: OnceLock<Mutex<VecDeque<LogEntry>>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static INIT: Once = Once::new();

#[derive(Debug, Clone, uniffi::Record)]
pub struct LogEntry {
    pub id: u64,
    pub timestamp: String,
    pub level: LogLevel,
    pub target: String,
    pub message: String,
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl From<&tracing::Level> for LogLevel {
    fn from(level: &tracing::Level) -> Self {
        match *level {
            tracing::Level::ERROR => LogLevel::Error,
            tracing::Level::WARN => LogLevel::Warn,
            tracing::Level::INFO => LogLevel::Info,
            _ => LogLevel::Debug,
        }
    }
}

fn buffer() -> &'static Mutex<VecDeque<LogEntry>> {
    BUFFER.get_or_init(|| Mutex::new(VecDeque::with_capacity(BUFFER_CAPACITY)))
}

struct MessageVisitor {
    message: String,
    extras: Vec<String>,
}

impl MessageVisitor {
    fn new() -> Self {
        Self {
            message: String::new(),
            extras: Vec::new(),
        }
    }

    fn finish(self) -> String {
        if self.extras.is_empty() {
            self.message
        } else {
            format!("{} {}", self.message, self.extras.join(" "))
        }
    }
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        } else {
            self.extras.push(format!("{}={:?}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.extras.push(format!("{}={}", field.name(), value));
        }
    }
}

pub struct RingBufferLayer;

impl<S: Subscriber> Layer<S> for RingBufferLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = MessageVisitor::new();
        event.record(&mut visitor);

        let entry = LogEntry {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            timestamp: crate::iso8601_now(),
            level: LogLevel::from(event.metadata().level()),
            target: event.metadata().target().to_string(),
            message: visitor.finish(),
        };

        if let Ok(mut buf) = buffer().lock() {
            if buf.len() >= BUFFER_CAPACITY {
                buf.pop_front();
            }
            buf.push_back(entry);
        }
    }
}

pub fn get_logs_since(since_id: u64) -> Vec<LogEntry> {
    let buf = match buffer().lock() {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    buf.iter()
        .filter(|e| e.id > since_id)
        .cloned()
        .collect()
}

pub fn clear_log_buffer() {
    if let Ok(mut buf) = buffer().lock() {
        buf.clear();
    }
}

pub fn init_logging() {
    INIT.call_once(|| {
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

        tracing_subscriber::registry()
            .with(RingBufferLayer)
            .init();
    });
}

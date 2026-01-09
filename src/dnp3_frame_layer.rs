use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

use crate::dnp3_service::RawFrame;

/// Custom tracing layer to capture DNP3 hex frames from library output
use crate::dnp3_service::ProtocolLogEntry;

pub struct Dnp3FrameLayer {
    pub frames: Arc<RwLock<VecDeque<RawFrame>>>,
    logs: Arc<RwLock<VecDeque<ProtocolLogEntry>>>,
    frame_counter: Arc<std::sync::atomic::AtomicU64>,
    log_counter: Arc<std::sync::atomic::AtomicU64>,
}

impl Dnp3FrameLayer {
    pub fn new(
        frames: Arc<RwLock<VecDeque<RawFrame>>>, 
        logs: Arc<RwLock<VecDeque<ProtocolLogEntry>>>,
        frame_counter: Arc<std::sync::atomic::AtomicU64>,
        log_counter: Arc<std::sync::atomic::AtomicU64>
    ) -> Self {
        Self { frames, logs, frame_counter, log_counter }
    }
}

impl<S: Subscriber> Layer<S> for Dnp3FrameLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: Context<'_, S>,
    ) {
        use tracing::field::{Field, Visit};
        
        struct HexVisitor {
            message: String,
            target: String,
        }
        
        impl Visit for HexVisitor {
            fn record_str(&mut self, field: &Field, value: &str) {
                if field.name() == "message" {
                    self.message.push_str(value);
                } else {
                    use std::fmt::Write;
                    write!(self.message, " {}={}", field.name(), value).ok();
                }
            }
            
            fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    use std::fmt::Write;
                    write!(self.message, "{:?}", value).ok();
                } else {
                    use std::fmt::Write;
                    write!(self.message, " {}={:?}", field.name(), value).ok();
                }
            }
        }
        
        let mut visitor = HexVisitor {
            message: String::new(),
            target: event.metadata().target().to_string(),
        };
        
        event.record(&mut visitor);
        
        // CRITICAL FIX: format!("{:?}", value) escapes newlines as "\n".
        // We must unescape them to let extract_hex_bytes work properly.
        let raw_msg = visitor.message.clone();
        let msg = raw_msg
            .replace("\\n", " ")
            .replace("\\r", " ")
            .replace("\\t", " ")
            .replace("\\\"", "\""); // Unescape quotes too just in case
        // Capture from dnp3 crate OR our own service
        if !visitor.target.starts_with("dnp3") && !visitor.target.contains("dnp3_tester") {
            return;
        }

        let msg_clean = raw_msg
            .replace("\\n", " ")
            .replace("\\r", " ")
            .replace("\\t", " ")
            .replace("\\\"", "\"");

        // 1. Try to extract hex frames
        if let Some(hex_data) = extract_hex_bytes(&msg_clean) {
            if hex_data.len() >= 2 {
                 let direction_str = if msg_clean.contains("TX") { "TX" } else { "RX" };
                 
                 let frames = self.frames.clone();
                 // ... push frame ...
                  if let Ok(mut q) = frames.try_write() {
                       if q.len() >= 1000 { q.pop_front(); }
                       let id = self.frame_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                       q.push_back(RawFrame {
                           id,
                           timestamp: chrono::Utc::now(),
                           direction: direction_str.to_string(),
                           data: hex_data,
                       });
                  }
                 // Frames are handled, return or continue?
                 // If it is a frame log, we might not want to duplicate it in System Log unless verbose.
                 // Let's return to avoid cluttering System Log with "Phys TX..." text which is redundant with Frame view.
                 return;
            }
        }

        // 2. Capture important System Logs (Warnings, Errors, Connectivity)
        let level = *event.metadata().level();
        let is_important = level <= tracing::Level::WARN 
            || msg_clean.contains("connected") 
            || msg_clean.contains("refused") 
            || msg_clean.contains("waiting");

        if is_important {
             // Use self.logs directly, no need to clone Arc for try_write
             if let Ok(mut q) = self.logs.try_write() {
                 if q.len() >= 1000 { q.pop_front(); }
                 
                 let direction = if level == tracing::Level::ERROR { "Error" } else { "System" };
                 let id = self.log_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                 
                 q.push_back(ProtocolLogEntry {
                      id,
                      timestamp: chrono::Utc::now(),
                      direction: direction.to_string(),
                      message: msg_clean, // Use the cleaned message
                      transaction_id: 0,
                 });
             }
        }
    }
}

/// Extract hex bytes from dnp3 library log message
/// Handle format: "PHYS TX - 15 bytes\n05 64 08 ..."
fn extract_hex_bytes(msg: &str) -> Option<Vec<u8>> {
    let mut all_bytes = Vec::new();

    // Tokenize the message by whitespace
    for word in msg.split_whitespace() {
        // Strip common punctuation if present (though log seems clean)
        let clean = word.trim_matches(|c| c == ',' || c == ':' || c == '[' || c == ']');
        
        // Attempt to parse 2-character hex strings
        if clean.len() == 2 {
            if let Ok(byte) = u8::from_str_radix(clean, 16) {
                all_bytes.push(byte);
            }
        }
    }

    // DNP3 frames ALWAYS start with 0x05 0x64.
    // This is the most robust way to find the frame within the log noise.
    if let Some(start_idx) = all_bytes.windows(2).position(|w| w == [0x05, 0x64]) {
        let frame_bytes = all_bytes[start_idx..].to_vec();
        
        // Basic length check (header is min 10 bytes)
        if frame_bytes.len() >= 10 {
            return Some(frame_bytes);
        }
    }

    None
}

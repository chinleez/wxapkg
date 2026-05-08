use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Quiet = 0,
    Normal = 1,
    Verbose = 2,
}

static LOG_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Normal as u8);

pub fn set(level: LogLevel) {
    LOG_LEVEL.store(level as u8, Ordering::Relaxed);
}

pub fn is_normal() -> bool {
    LOG_LEVEL.load(Ordering::Relaxed) >= LogLevel::Normal as u8
}

pub fn is_verbose() -> bool {
    LOG_LEVEL.load(Ordering::Relaxed) >= LogLevel::Verbose as u8
}

pub fn info(msg: impl AsRef<str>) {
    if is_normal() {
        println!("{}", msg.as_ref());
    }
}

pub fn verbose(msg: impl AsRef<str>) {
    if is_verbose() {
        println!("{}", msg.as_ref());
    }
}

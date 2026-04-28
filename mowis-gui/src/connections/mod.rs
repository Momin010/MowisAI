#[cfg(unix)]
pub mod unix;

pub mod tcp;

#[cfg(target_os = "windows")]
pub mod pipe;

#[cfg(target_os = "windows")]
pub mod pipe_bridge;

#[cfg(unix)]
pub use unix::UnixSocketConnection;

//! The transfer engine: what turns an address into a file on disk, and everything a download
//! manager promises about how -- probing what the server allows, writing in segments through
//! several connections, resuming what was interrupted, and pacing the whole. It knows nothing
//! of a window; the application drives it through [`Engine`] and listens through events.
//! See spec/engine.md.

pub mod client;
pub mod control;
pub mod error;
pub mod limiter;
pub mod probe;
pub mod segments;
pub mod settings;
pub mod task;
#[cfg(test)]
pub mod testing;
pub mod worker;
pub mod writer;

pub use control::Control;
pub use error::{Error, Result};
pub use limiter::Limiter;
pub use probe::{Probe, probe};
pub use segments::{Plan, Segment, Span};
pub use settings::{Connections, HttpVersion, Settings};
pub use task::{Finished, Handle, Progress, Request};
pub use writer::Writer;

//! The transfer engine: what turns an address into a file on disk, and everything a download
//! manager promises about how -- probing what the server allows, writing in segments through
//! several connections, resuming what was interrupted, and pacing the whole. It knows nothing
//! of a window; the application drives it through [`Engine`] and listens through events, and
//! nothing outside this module reaches past `engine::`. See spec/engine.md.

// TODO: the window drives downloads through this module, but not yet the rest of what it can
// do -- checksums, mirrors, ranges, the limits and counts a settings window will bind to -- so
// those items are unreached from the binary. The allow narrows as the window grows.
#![allow(dead_code, unused_imports)]

pub mod client;
pub mod control;
pub mod error;
pub mod limiter;
#[cfg(test)]
mod mirror;
pub mod probe;
pub mod queue;
pub mod segments;
pub mod settings;
pub mod task;
#[cfg(test)]
pub mod testing;
pub mod verify;
pub mod worker;
pub mod writer;

pub use control::Control;
pub use error::{Error, Result};
pub use limiter::Limiter;
pub use probe::{Probe, probe};
pub use queue::{Engine, EngineSettings, Event, Snapshot, Status, TaskId};
pub use segments::{Plan, Segment, Span};
pub use settings::{Connections, HttpVersion, Settings};
pub use task::{Finished, Handle, Progress, Request};
pub use verify::Checksum;
pub use writer::Writer;

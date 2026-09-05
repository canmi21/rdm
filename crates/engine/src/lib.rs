//! The transfer engine: what turns an address into a file on disk, and everything a download
//! manager promises about how -- probing what the server allows, writing in segments through
//! several connections, resuming what was interrupted, and pacing the whole. It knows nothing
//! of a window; the application drives it through [`Engine`] and listens through events.
//! See spec/engine.md.

pub mod segments;
pub mod settings;

pub use segments::{Plan, Segment, Span};
pub use settings::{Connections, HttpVersion, Settings};

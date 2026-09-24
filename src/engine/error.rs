//! What can go wrong with a download, as the window will want to say it: one variant per
//! thing the user can act on, and the transport's own error kept underneath for the log.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("the address is not a URL: {0}")]
	Address(#[from] url::ParseError),
	#[error("the address must be http or https, not {0}")]
	Scheme(String),
	#[error("network: {0}")]
	Http(#[from] reqwest::Error),
	#[error("the server answered {status}")]
	Refused { status: u16 },
	/// The server said it was too busy for this connection -- 429 or 503 -- and, when it said,
	/// how long to wait: what a server limiting how many connections it takes usually answers,
	/// so the scheduler reads it as a sign to open fewer. See spec/engine.md.
	#[error("the server answered {status}")]
	Busy { status: u16, retry_after: Option<std::time::Duration> },
	#[error("the server does not serve byte ranges, so this download cannot be resumed or split")]
	NoRanges,
	#[error("the file is {size} bytes, over the limit of {limit}")]
	TooLarge { size: u64, limit: u64 },
	#[error("the file changed on the server since the download began")]
	Changed,
	#[error("the server sent {got} bytes where {want} were expected")]
	ShortBody { want: u64, got: u64 },
	#[error("the requested range lies outside the file")]
	OutOfRange,
	#[error("{path}: {source}")]
	Disk {
		path: PathBuf,
		#[source]
		source: std::io::Error,
	},
	#[error("the checksum does not match: expected {expected}, computed {computed}")]
	Checksum { expected: String, computed: String },
	#[error("gave up after {tries} tries: {last}")]
	GaveUp { tries: u32, last: Box<Error> },
	#[error("the download was cancelled")]
	Cancelled,
	#[error("the control file beside the partial download is not one this build can read")]
	Control,
}

pub type Result<T> = std::result::Result<T, Error>;

/// A look at an address that failed, as the window says it: a sentence short enough for a line of
/// the sheet, and everything the transport said, for whoever opens the details. See spec/ui.md.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
	pub summary: String,
	pub detail: String,
}

impl Error {
	/// What went wrong in words a person can act on, one sentence: a status with its reason, a
	/// server that could not be reached, a wait that ran out. The whole text is `detail`.
	pub fn summary(&self) -> String {
		match self {
			Error::Address(_) => "That is not a web address.".to_owned(),
			Error::Scheme(scheme) => format!("Only http and https can be downloaded, not {scheme}."),
			Error::Http(error) if error.is_timeout() => "The server took too long to answer.".to_owned(),
			Error::Http(error) if error.is_connect() => "The server could not be reached.".to_owned(),
			Error::Http(error) if error.is_redirect() => "The address redirects too many times.".to_owned(),
			Error::Http(_) => "The connection failed before the server answered.".to_owned(),
			Error::Refused { status } | Error::Busy { status, .. } => {
				match reqwest::StatusCode::from_u16(*status).ok().and_then(|code| code.canonical_reason()) {
					Some(reason) => format!("The server answered {status} {reason}."),
					None => format!("The server answered {status}."),
				}
			}
			Error::GaveUp { last, .. } => last.summary(),
			other => {
				let text = other.to_string();
				let mut chars = text.chars();
				let first: String = chars.next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
				format!("{first}{}.", chars.as_str())
			}
		}
	}

	/// The error's own text with every cause under it, each said once: what the transport really
	/// said, which the summary leaves out.
	pub fn detail(&self) -> String {
		let mut detail = self.to_string();
		let mut source = std::error::Error::source(self);
		while let Some(cause) = source {
			let said = cause.to_string();
			if !detail.contains(&said) {
				detail.push_str(": ");
				detail.push_str(&said);
			}
			source = cause.source();
		}
		detail
	}

	pub fn failure(&self) -> Failure {
		Failure { summary: self.summary(), detail: self.detail() }
	}

	/// Whether trying again could help: a transport failure or a short body might have been the
	/// network; a refusal, a changed file or a full disk will not go away.
	pub fn is_transient(&self) -> bool {
		match self {
			Error::Http(_) | Error::ShortBody { .. } | Error::Busy { .. } => true,
			Error::Refused { status } => matches!(status, 408 | 429 | 500 | 502 | 503 | 504),
			_ => false,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_failure_is_a_sentence_for_the_sheet_and_the_whole_text_for_the_details() {
		assert_eq!(Error::Refused { status: 403 }.summary(), "The server answered 403 Forbidden.");
		assert_eq!(Error::Refused { status: 599 }.summary(), "The server answered 599.");
		assert_eq!(Error::OutOfRange.summary(), "The requested range lies outside the file.");
		let disk = Error::Disk {
			path: PathBuf::from("/tmp/x"),
			source: std::io::Error::other("no space left"),
		};
		assert_eq!(disk.detail(), "/tmp/x: no space left", "a cause already in the text is said once");
		let given_up = Error::GaveUp { tries: 3, last: Box::new(Error::Refused { status: 503 }) };
		assert_eq!(given_up.summary(), "The server answered 503 Service Unavailable.");
	}
}

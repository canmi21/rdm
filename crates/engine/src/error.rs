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

impl Error {
	/// Whether trying again could help: a transport failure or a short body might have been the
	/// network; a refusal, a changed file or a full disk will not go away.
	pub fn is_transient(&self) -> bool {
		match self {
			Error::Http(_) | Error::ShortBody { .. } => true,
			Error::Refused { status } => matches!(status, 408 | 429 | 500 | 502 | 503 | 504),
			_ => false,
		}
	}
}

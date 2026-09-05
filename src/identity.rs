//! Who this application is to the operating system. One home for the three words the bundle
//! identifier and the state directory are both spelled from. See spec/state.md.

/// Reverse-DNS of the domain kept for software, then the application: `app.canmi.rdm`.
pub const QUALIFIER: &str = "app";
pub const ORGANIZATION: &str = "canmi";
pub const APPLICATION: &str = "rdm";

/// What `Info.plist` carries, read by the bundle task out of this source; what the window is
/// named to a Linux desktop, handed to GPUI as its app id; and the three words that name the
/// state directory.
pub const BUNDLE_ID: &str = "app.canmi.rdm";

/// Which build this is, read at compile time from the release workflow's environment: the run
/// number, which only grows, and the commit. None in a build made by hand. Every build of one
/// day's version differs in these alone, and the update check compares the number and nothing
/// else. See spec/release.md.
pub const BUILD: Option<&str> = option_env!("GITHUB_RUN_NUMBER");
pub const COMMIT: Option<&str> = option_env!("GITHUB_SHA");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_bundle_id_is_the_three_words_joined() {
		assert_eq!(BUNDLE_ID, format!("{QUALIFIER}.{ORGANIZATION}.{APPLICATION}"));
	}
}

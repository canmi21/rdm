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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_bundle_id_is_the_three_words_joined() {
		assert_eq!(BUNDLE_ID, format!("{QUALIFIER}.{ORGANIZATION}.{APPLICATION}"));
	}
}

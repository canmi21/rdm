//! Who this application is to the operating system. One home for the three words the bundle
//! identifier and the state directory are both spelled from. See spec/state.md.

/// Reverse-DNS of the domain kept for software, then the application: `app.canmi.rdm`.
pub const QUALIFIER: &str = "app";
pub const ORGANIZATION: &str = "canmi";
pub const APPLICATION: &str = "rdm";

/// What `Info.plist` carries. Read by the bundle task out of this source, not by Rust code, which
/// is why the compiler sees it unused; it also names the state directory through the three words.
#[cfg_attr(not(test), expect(dead_code))]
pub const BUNDLE_ID: &str = "app.canmi.rdm";

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_bundle_id_is_the_three_words_joined() {
		assert_eq!(BUNDLE_ID, format!("{QUALIFIER}.{ORGANIZATION}.{APPLICATION}"));
	}
}

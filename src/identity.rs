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

/// The name in full, what the `.app` is called and the menu bar and About show; and the name
/// the Dock, Spotlight and the window title show, one word like the system's own. The code
/// and the binary stay `rdm`. The bundle task reads both out of this source. See
/// spec/packaging.md.
pub const NAME: &str = "Refined Download Manager";
pub const DISPLAY_NAME: &str = "Downloads";

/// Which build this is, read at compile time from the release workflow's environment: the run
/// number, which only grows, and the commit. None in a build made by hand. Every build of one
/// day's version differs in these alone, and the update check compares the number and nothing
/// else. See spec/release.md.
pub const BUILD: Option<&str> = option_env!("GITHUB_RUN_NUMBER");
pub const COMMIT: Option<&str> = option_env!("GITHUB_SHA");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where the builds are published, `user/repo` on GitHub, and which of them this binary is: the
/// system and the architecture as the release names its files. See spec/release.md.
pub const REPOSITORY: &str = "canmi21/rdm";
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const TARGET: &str = "macos-arm64";
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
pub const TARGET: &str = "windows-x64";
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub const TARGET: &str = "linux-x64";
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub const TARGET: &str = "linux-arm64";
#[cfg(not(any(
	all(target_os = "macos", target_arch = "aarch64"),
	all(target_os = "windows", target_arch = "x86_64"),
	all(target_os = "linux", target_arch = "x86_64"),
	all(target_os = "linux", target_arch = "aarch64"),
)))]
pub const TARGET: &str = "unpublished";

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_bundle_id_is_the_three_words_joined() {
		assert_eq!(BUNDLE_ID, format!("{QUALIFIER}.{ORGANIZATION}.{APPLICATION}"));
	}

	#[test]
	fn the_executable_is_named_what_the_window_is() {
		assert_eq!(env!("CARGO_BIN_NAME"), DISPLAY_NAME, "Cargo.toml's [[bin]] name");
	}
}

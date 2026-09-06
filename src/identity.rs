//! Who this application is to the operating system. One home for the three words the bundle
//! identifier and the state directory are both spelled from. See spec/state.md.

/// Reverse-DNS of the domain kept for software, then the application: `app.canmi.rdm`.
pub const QUALIFIER: &str = "app";
pub const ORGANIZATION: &str = "canmi";
pub const APPLICATION: &str = "rdm";

/// What `Info.plist` carries and what a Linux desktop matches the window to, for the build that
/// is published. `.mise/tasks/bundle` reads this line out of this source with a regular
/// expression, so it stays a plain literal; a running build answers to [`id`] instead, which is
/// this and the suffix below.
pub const BUNDLE_ID: &str = "app.canmi.rdm";

/// What separates a development build from an installed one. A debug build keeps its own state
/// directory, its own `config.json` and its own database: everything under the three words is
/// spelled with `.dev` after the last of them, so `mise run dev` cannot disturb what the
/// installed application has, and neither can rewrite the other's. The downloads folder is the
/// user's own and is shared, since a download folder nobody looks in is no use to either. The
/// discriminator is `debug_assertions`, the same one the control socket answers to, and the dev
/// profile leaves it on. See spec/state.md.
#[cfg(debug_assertions)]
pub const SUFFIX: &str = ".dev";
#[cfg(not(debug_assertions))]
pub const SUFFIX: &str = "";

/// What this build answers to: the bundle identifier as published, or that with `.dev` after it
/// in a development build. This is what GPUI is handed as the app id, what the system is told a
/// notification comes from, and what Settings shows -- so a window that is a development build
/// says so where somebody would look.
pub fn id() -> String {
	format!("{BUNDLE_ID}{SUFFIX}")
}

/// The last of the three words as this build spells it, which is what names the directory the
/// state lives in. `APPLICATION` itself never moves: the release's files and the CDN path are
/// spelled from it.
pub fn instance() -> String {
	format!("{APPLICATION}{SUFFIX}")
}

/// The name in full, what the `.app` is called and the menu bar and About show; and the name
/// the Dock, Spotlight and the window title show, one word like the system's own. The code
/// and the binary stay `rdm`. The bundle task reads both out of this source. See
/// spec/packaging.md.
pub const NAME: &str = "Refined Download Manager";
pub const DISPLAY_NAME: &str = "Downloads";

/// The last nightly whose files were called by the name in full -- `Refined Download
/// Manager.app` on macOS through build 11, `Refined Download Manager.exe` on Windows through
/// this one -- before every file became `Downloads`. A build that finds itself under the old
/// name after one of these renames itself once; a later one leaves whatever name it has, since
/// that is the user's. See spec/release.md.
pub const LEGACY_NAME_UNTIL: u64 = 17;

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

	/// A development build and an installed one must not meet in the same directory, and the
	/// tests are a development build, so this reads the suffix rather than asserting it away.
	#[test]
	fn a_development_build_answers_to_its_own_id_and_its_own_directory() {
		assert_eq!(id(), format!("{BUNDLE_ID}{SUFFIX}"));
		assert_eq!(instance(), format!("{APPLICATION}{SUFFIX}"));
		assert_eq!(SUFFIX, ".dev", "a test binary is a debug build");
		assert_eq!(id(), "app.canmi.rdm.dev");
		assert_ne!(id(), BUNDLE_ID, "so it cannot be handed the installed build's directory");
	}

	#[test]
	fn the_executable_is_named_what_the_window_is() {
		assert_eq!(env!("CARGO_BIN_NAME"), DISPLAY_NAME, "Cargo.toml's [[bin]] name");
	}
}

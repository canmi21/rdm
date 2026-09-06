//! What this application calls itself to a server, and the three other things it can call itself.
//!
//! The honest answer is `rdm/<version>`, and it is what is sent unless somebody says otherwise.
//! The others are a disguise, and the disguise is offered because some servers hand a different
//! file, a slower one or none at all to a client that admits to being a download manager. Whether
//! to wear one is the user's business; this only makes the three that are worth wearing easy to
//! pick, so nobody has to go and find a plausible string and paste it in.
//!
//! Two of the three are shown at a time, and never this system's own. A macOS machine claiming to
//! be a macOS machine is not a disguise, and a list that offers it as one is a list that has not
//! thought about what it is for. See spec/engine.md.

use serde::{Deserialize, Serialize};

/// The browser the disguises claim to be, and the version they claim. One number in one place:
/// a disguise with a version three years old is a disguise, and not a good one. Chrome freezes
/// everything after the major, so this is the whole of what changes.
const CHROME: u32 = 153;

/// Which of the four is sent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Agent {
	/// `rdm/<version>`: what this is.
	#[default]
	Own,
	/// Chrome on Windows.
	Windows,
	/// Chrome on macOS.
	Macos,
	/// Chrome on Linux.
	Linux,
	/// Whatever is written in the field beside it.
	Custom,
}

impl Agent {
	/// What Settings offers, in order: this application, the two disguises that are not this
	/// system, and a field of one's own.
	pub fn offered() -> Vec<Agent> {
		let mut offered = vec![Agent::Own];
		offered.extend(Agent::DISGUISES.into_iter().filter(|agent| *agent != Agent::this_system()));
		offered.push(Agent::Custom);
		offered
	}

	const DISGUISES: [Agent; 3] = [Agent::Windows, Agent::Macos, Agent::Linux];

	/// The disguise that would be this machine telling the truth, which is the one left out.
	fn this_system() -> Agent {
		if cfg!(target_os = "macos") {
			Agent::Macos
		} else if cfg!(windows) {
			Agent::Windows
		} else {
			Agent::Linux
		}
	}

	pub fn name(self) -> &'static str {
		match self {
			Agent::Own => "This application",
			Agent::Windows => "Chrome on Windows",
			Agent::Macos => "Chrome on macOS",
			Agent::Linux => "Chrome on Linux",
			Agent::Custom => "Something else",
		}
	}

	/// The string itself. `own` is what this application calls itself and `written` is the field,
	/// both passed in rather than reached for, so this stays a table.
	pub fn string(self, own: &str, written: &str) -> String {
		let chrome = |platform: &str| {
			format!(
				"Mozilla/5.0 ({platform}) AppleWebKit/537.36 (KHTML, like Gecko) \
				 Chrome/{CHROME}.0.0.0 Safari/537.36"
			)
		};
		match self {
			Agent::Own => own.to_owned(),
			Agent::Windows => chrome("Windows NT 10.0; Win64; x64"),
			// Chrome froze this at 10_15_7 years ago and still sends it from every Mac; a
			// disguise that said the real version would be the one that stood out.
			Agent::Macos => chrome("Macintosh; Intel Mac OS X 10_15_7"),
			Agent::Linux => chrome("X11; Linux x86_64"),
			Agent::Custom => written.to_owned(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Two disguises are offered and never the one this machine would be telling the truth with.
	#[test]
	fn the_disguise_this_system_would_not_be_is_the_one_left_out() {
		let offered = Agent::offered();
		assert_eq!(offered.len(), 4, "this application, two disguises, and one's own");
		assert_eq!(offered[0], Agent::Own);
		assert_eq!(offered[3], Agent::Custom);
		assert!(!offered.contains(&Agent::this_system()), "which would not be a disguise");
		#[cfg(target_os = "macos")]
		assert_eq!(&offered[1..3], [Agent::Windows, Agent::Linux]);
	}

	#[test]
	fn a_disguise_reads_as_the_browser_it_claims_to_be() {
		let windows = Agent::Windows.string("rdm/0.0.0", "");
		assert!(windows.starts_with("Mozilla/5.0 (Windows NT 10.0; Win64; x64)"));
		assert!(windows.contains(&format!("Chrome/{CHROME}.0.0.0")), "and a version worth claiming");
		assert!(Agent::Macos.string("rdm/0.0.0", "").contains("Mac OS X 10_15_7"), "as Chrome sends it");
		assert!(Agent::Linux.string("rdm/0.0.0", "").contains("X11; Linux x86_64"));
	}

	#[test]
	fn the_honest_answer_is_the_default_and_the_field_is_the_other_one() {
		assert_eq!(Agent::default(), Agent::Own);
		assert_eq!(Agent::Own.string("rdm/1.2.3", "ignored"), "rdm/1.2.3");
		assert_eq!(Agent::Custom.string("rdm/1.2.3", "curl/8"), "curl/8");
	}
}

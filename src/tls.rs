//! Which crypto rustls runs on, chosen here rather than left to each crate that speaks TLS.
//!
//! rustls asks the process for a default provider, and every crate that wants one would otherwise
//! bring its own: reqwest's `default-tls` brings AWS-LC, the resolver's `https-ring` brings ring,
//! and the binary would carry two crypto libraries doing the same job -- with whichever installed
//! itself first being the one in use, a coin toss written in the dependency graph rather than in
//! the source. So both are told to bring nothing, in `Cargo.toml`, and the choice they were
//! denied is made here by a feature.
//!
//! **AWS-LC by default**, which is what the release builds carry and what the platforms this is
//! shipped for are happiest with; **ring** with `--no-default-features --features ring`, for a
//! build with no C toolchain to spend on cmake. Both are additive, as Cargo features are, so
//! AWS-LC wins when both are on: a `--features ring` that forgot `--no-default-features` then
//! builds the default rather than failing to build at all.
//!
//! Nothing here is OpenSSL, and nothing in the tree is: there is no `openssl-sys` and no
//! `native-tls`. AWS-LC is a fork of BoringSSL, which was a fork of OpenSSL, so the lineage is
//! there in the C -- but it is not the OpenSSL library, and no `libssl` is linked. See
//! spec/engine.md.

use std::sync::Once;

#[cfg(feature = "aws-lc-rs")]
use rustls::crypto::aws_lc_rs as chosen;
#[cfg(all(feature = "ring", not(feature = "aws-lc-rs")))]
use rustls::crypto::ring as chosen;

#[cfg(not(any(feature = "aws-lc-rs", feature = "ring")))]
compile_error!("one of the aws-lc-rs and ring features has to be on: there is no third way to speak TLS");

/// Installs the provider, once, whoever asks first. Every client this application builds calls it
/// before building, so a test that never runs `main` gets one too, and a provider is never
/// installed after the connection that needed it.
pub fn install() {
	static ONCE: Once = Once::new();
	ONCE.call_once(|| {
		// Failure means a provider was already installed, which the Once is what rules out.
		let _ = chosen::default_provider().install_default();
	});
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Asking twice is what happens: two clients, a resolver, and no saying in what order.
	#[test]
	fn the_provider_is_installed_once_and_asking_again_is_no_trouble() {
		install();
		install();
		assert!(
			rustls::crypto::CryptoProvider::get_default().is_some(),
			"something is there to encrypt with"
		);
	}
}

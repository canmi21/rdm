//! Files compiled into the binary: the icons, and nothing else so far.

use std::borrow::Cow;

use anyhow::Result;
use gpui::{AssetSource, SharedString};

/// Lucide icons under assets/icons, ISC licensed; the licence sits beside them.
#[derive(rust_embed::Embed)]
#[folder = "assets"]
pub struct Assets;

impl AssetSource for Assets {
	fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
		Ok(Self::get(path).map(|file| file.data))
	}

	fn list(&self, path: &str) -> Result<Vec<SharedString>> {
		Ok(Self::iter().filter(|p| p.starts_with(path)).map(SharedString::from).collect())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn icons_are_embedded() {
		let listed = Assets.list("icons/").unwrap();
		assert!(listed.iter().any(|p| p.as_ref() == "icons/plus.svg"), "{listed:?}");
		assert!(Assets.load("icons/plus.svg").unwrap().is_some());
	}
}

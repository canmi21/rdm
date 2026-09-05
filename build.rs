//! Two things a build needs settled before the compiler runs. The build number and commit are
//! read from the environment by `option_env!` in src/identity.rs, and Cargo has to be told to
//! rebuild when they change, or a binary built after the environment moved would carry the old
//! ones. On Windows the icon and the two names live inside the executable, and winresource
//! writes the resource that puts them there: the Task Manager shows the description, the
//! properties sheet the product. See spec/release.md.

fn main() {
	println!("cargo:rerun-if-env-changed=GITHUB_RUN_NUMBER");
	println!("cargo:rerun-if-env-changed=GITHUB_SHA");
	println!("cargo:rerun-if-changed=assets/icon-256.ico");
	#[cfg(windows)]
	winresource::WindowsResource::new()
		.set_icon("assets/icon-256.ico")
		.set("ProductName", "Refined Download Manager")
		.set("FileDescription", "Downloads")
		.compile()
		.expect("embed the icon");
}

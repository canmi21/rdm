//! Putting a downloaded build where this one runs from. Each system installs one way: the
//! macOS bundle is swapped for the one in the dmg, the Windows executable is replaced by the
//! one in the zip with the running file moved aside, the Linux AppImage is renamed over, the
//! Linux binary is renamed over with the one from the tarball. A rename is the whole trick: a
//! running program keeps its old inode, or on Windows its old name, and the new file takes the
//! path. Nothing here touches the running file before the new one is on disk beside it. See
//! spec/release.md.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Where this binary runs from, and so what an update replaces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
	/// macOS: the `.app` this binary is inside.
	Bundle(PathBuf),
	/// Windows: the executable itself.
	Exe(PathBuf),
	/// Linux: the AppImage this process was mounted from, which its runtime names in `APPIMAGE`.
	AppImage(PathBuf),
	/// Linux: a bare binary, as the tarball's `install.sh` put it.
	Binary(PathBuf),
}

impl Place {
	/// The kind of release file that replaces this place, as `latest.json` names kinds.
	pub fn kind(&self) -> &'static str {
		match self {
			Place::Bundle(_) => "dmg",
			Place::Exe(_) => "zip",
			Place::AppImage(_) => "AppImage",
			Place::Binary(_) => "tar.gz",
		}
	}
}

/// Where this process runs from.
pub fn place() -> Result<Place, String> {
	let exe = std::env::current_exe().map_err(|e| format!("where this runs from: {e}"))?;
	let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
	locate(&exe, appimage.as_deref(), std::env::consts::OS)
}

/// What `place` decides, from the executable's path alone. A build run from its build tree is
/// not replaced: that is the developer's, and Cargo's to overwrite. On macOS the bundle is the
/// third ancestor of the executable, `X.app/Contents/MacOS/X`, and one on a mounted disk image
/// is running from the installer rather than installed.
pub fn locate(exe: &Path, appimage: Option<&Path>, os: &str) -> Result<Place, String> {
	if exe.components().any(|c| c.as_os_str() == "target") {
		return Err("this build runs from its build tree and is not replaced".to_owned());
	}
	if let Some(appimage) = appimage {
		return Ok(Place::AppImage(appimage.to_path_buf()));
	}
	match os {
		"macos" => {
			let bundle = exe
				.ancestors()
				.nth(3)
				.filter(|p| p.extension().is_some_and(|e| e == "app"))
				.ok_or_else(|| "this binary is not inside an application bundle".to_owned())?;
			if bundle.starts_with("/Volumes") {
				return Err(
					"drag the application into Applications first; it runs from the installer".to_owned(),
				);
			}
			Ok(Place::Bundle(bundle.to_path_buf()))
		}
		"windows" => Ok(Place::Exe(exe.to_path_buf())),
		_ => Ok(Place::Binary(exe.to_path_buf())),
	}
}

/// Installs the downloaded file over the place, and says what to launch afterwards.
pub fn install(file: &Path, place: &Place) -> Result<PathBuf, String> {
	match place {
		Place::Bundle(bundle) => install_bundle(file, bundle),
		Place::Exe(exe) => install_exe(file, exe),
		Place::AppImage(path) => {
			let staged = beside(path, "update");
			std::fs::copy(file, &staged).map_err(|e| format!("copy the AppImage: {e}"))?;
			executable(&staged)?;
			std::fs::rename(&staged, path).map_err(|e| format!("replace {}: {e}", path.display()))?;
			Ok(path.clone())
		}
		Place::Binary(path) => install_binary(file, path),
	}
}

/// Starts the installed build, then the caller quits this one.
pub fn launch(path: &Path) -> Result<(), String> {
	let mut command = if cfg!(target_os = "macos") {
		let mut open = Command::new("open");
		open.arg("-n").arg(path);
		open
	} else {
		Command::new(path)
	};
	command.spawn().map(|_| ()).map_err(|e| format!("start {}: {e}", path.display()))
}

/// A hidden sibling of the path, for a file on its way in or out.
fn beside(path: &Path, tag: &str) -> PathBuf {
	let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
	path.with_file_name(format!(".{name}.{tag}"))
}

fn executable(path: &Path) -> Result<(), String> {
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
			.map_err(|e| format!("mark {} executable: {e}", path.display()))?;
	}
	let _ = path;
	Ok(())
}

fn run(command: &mut Command, what: &str) -> Result<(), String> {
	let output = command.output().map_err(|e| format!("{what}: {e}"))?;
	if output.status.success() {
		Ok(())
	} else {
		Err(format!("{what}: {}", String::from_utf8_lossy(&output.stderr).trim()))
	}
}

/// The dmg is mounted, its bundle copied beside the installed one with `ditto`, which keeps
/// what a plain copy loses, then the two are swapped by rename and the old one removed. The
/// old bundle keeps running from its inode, whatever it is called meanwhile.
fn install_bundle(dmg: &Path, bundle: &Path) -> Result<PathBuf, String> {
	let mount = beside(dmg, "mount");
	let _ = std::fs::create_dir_all(&mount);
	run(
		Command::new("hdiutil")
			.args(["attach", "-nobrowse", "-readonly", "-noautoopen", "-quiet", "-mountpoint"])
			.arg(&mount)
			.arg(dmg),
		"mount the disk image",
	)?;
	let result = (|| {
		let fresh = std::fs::read_dir(&mount)
			.map_err(|e| format!("read the disk image: {e}"))?
			.filter_map(|entry| entry.ok().map(|e| e.path()))
			.find(|p| p.extension().is_some_and(|e| e == "app"))
			.ok_or_else(|| "no application in the disk image".to_owned())?;
		let staged = beside(bundle, "update");
		let _ = std::fs::remove_dir_all(&staged);
		run(Command::new("ditto").arg(&fresh).arg(&staged), "copy the application")?;
		let old = beside(bundle, "old");
		let _ = std::fs::remove_dir_all(&old);
		std::fs::rename(bundle, &old).map_err(|e| format!("move the old application aside: {e}"))?;
		if let Err(e) = std::fs::rename(&staged, bundle) {
			let _ = std::fs::rename(&old, bundle);
			return Err(format!("put the new application in place: {e}"));
		}
		let _ = std::fs::remove_dir_all(&old);
		Ok(bundle.to_path_buf())
	})();
	let _ = Command::new("hdiutil").args(["detach", "-quiet"]).arg(&mount).output();
	let _ = std::fs::remove_dir(&mount);
	result
}

/// The zip holds the executable alone. Windows keeps a running executable's file locked
/// against deletion and writing but not against renaming, so `self_replace` moves the running
/// file aside and puts the new one under its name; the moved one goes when it closes.
#[cfg(windows)]
fn install_exe(zip: &Path, exe: &Path) -> Result<PathBuf, String> {
	let file = std::fs::File::open(zip).map_err(|e| format!("open the zip: {e}"))?;
	let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("read the zip: {e}"))?;
	let index = (0..archive.len())
		.find(|&i| archive.by_index(i).is_ok_and(|f| f.name().ends_with(".exe")))
		.ok_or_else(|| "no executable in the zip".to_owned())?;
	let staged = beside(exe, "update");
	{
		let mut entry = archive.by_index(index).map_err(|e| format!("read the zip: {e}"))?;
		let mut out =
			std::fs::File::create(&staged).map_err(|e| format!("write the executable: {e}"))?;
		std::io::copy(&mut entry, &mut out).map_err(|e| format!("write the executable: {e}"))?;
	}
	self_replace::self_replace(&staged).map_err(|e| format!("replace the executable: {e}"))?;
	let _ = std::fs::remove_file(&staged);
	Ok(exe.to_path_buf())
}

#[cfg(not(windows))]
fn install_exe(_zip: &Path, exe: &Path) -> Result<PathBuf, String> {
	Err(format!("{} is a Windows executable and this is not Windows", exe.display()))
}

/// The tarball holds `rdm/rdm` beside the desktop entry and the icon; the binary alone is
/// taken, since the entry and the icon are where `install.sh` put them and did not change.
#[cfg(target_os = "linux")]
fn install_binary(tarball: &Path, binary: &Path) -> Result<PathBuf, String> {
	let file = std::fs::File::open(tarball).map_err(|e| format!("open the tarball: {e}"))?;
	let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
	let staged = beside(binary, "update");
	let mut found = false;
	for entry in archive.entries().map_err(|e| format!("read the tarball: {e}"))? {
		let mut entry = entry.map_err(|e| format!("read the tarball: {e}"))?;
		let path = entry.path().map_err(|e| format!("read the tarball: {e}"))?.into_owned();
		if path.file_name().is_some_and(|n| n == "rdm") && entry.header().entry_type().is_file() {
			let mut out = std::fs::File::create(&staged).map_err(|e| format!("write the binary: {e}"))?;
			std::io::copy(&mut entry, &mut out).map_err(|e| format!("write the binary: {e}"))?;
			found = true;
			break;
		}
	}
	if !found {
		return Err("no binary in the tarball".to_owned());
	}
	executable(&staged)?;
	std::fs::rename(&staged, binary).map_err(|e| format!("replace {}: {e}", binary.display()))?;
	Ok(binary.to_path_buf())
}

#[cfg(not(target_os = "linux"))]
fn install_binary(_tarball: &Path, binary: &Path) -> Result<PathBuf, String> {
	Err(format!("{} is a Linux binary and this is not Linux", binary.display()))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_bundle_is_the_third_ancestor_and_one_on_a_volume_is_the_installer() {
		let exe = Path::new("/Applications/Downloads.app/Contents/MacOS/Downloads");
		assert_eq!(
			locate(exe, None, "macos"),
			Ok(Place::Bundle(PathBuf::from("/Applications/Downloads.app")))
		);
		let exe = Path::new("/Volumes/Refined Installer/Downloads.app/Contents/MacOS/Downloads");
		assert!(locate(exe, None, "macos").unwrap_err().contains("Applications first"));
		assert!(locate(Path::new("/usr/local/bin/Downloads"), None, "macos").is_err());
	}

	#[test]
	fn windows_is_the_executable_and_linux_the_appimage_or_the_binary() {
		let exe = Path::new("C:\\Users\\x\\Downloads\\Refined Download Manager.exe");
		assert_eq!(locate(exe, None, "windows"), Ok(Place::Exe(exe.to_path_buf())));
		let mounted = Path::new("/tmp/.mount_rdmXYZ/usr/bin/rdm");
		let image = Path::new("/home/x/Applications/rdm-nightly-linux-x64.AppImage");
		assert_eq!(locate(mounted, Some(image), "linux"), Ok(Place::AppImage(image.to_path_buf())));
		let bin = Path::new("/home/x/.local/bin/rdm");
		assert_eq!(locate(bin, None, "linux"), Ok(Place::Binary(bin.to_path_buf())));
		assert_eq!(
			(Place::Bundle(PathBuf::new()).kind(), Place::Exe(PathBuf::new()).kind()),
			("dmg", "zip")
		);
		assert_eq!(
			(Place::AppImage(PathBuf::new()).kind(), Place::Binary(PathBuf::new()).kind()),
			("AppImage", "tar.gz")
		);
	}

	/// The mechanics on a bundle of one file, from a disk image made here.
	#[cfg(target_os = "macos")]
	#[test]
	fn a_bundle_is_swapped_for_the_one_in_the_disk_image() {
		let dir = crate::testing::scratch("install-bundle");
		let make = |root: &Path, text: &str| {
			let bin = root.join("Downloads.app/Contents/MacOS");
			std::fs::create_dir_all(&bin).unwrap();
			std::fs::write(bin.join("Downloads"), text).unwrap();
		};
		let installed = dir.join("Applications");
		make(&installed, "old");
		let source = dir.join("source");
		make(&source, "new");
		let dmg = dir.join("update.dmg");
		let made = Command::new("hdiutil")
			.args(["create", "-quiet", "-volname", "Test", "-srcfolder"])
			.arg(&source)
			.args(["-ov", "-format", "UDZO"])
			.arg(&dmg)
			.status()
			.unwrap();
		assert!(made.success());
		let bundle = installed.join("Downloads.app");
		let launch = install(&dmg, &Place::Bundle(bundle.clone())).unwrap();
		assert_eq!(launch, bundle);
		assert_eq!(std::fs::read_to_string(bundle.join("Contents/MacOS/Downloads")).unwrap(), "new");
		assert!(!beside(&bundle, "old").exists() && !beside(&bundle, "update").exists());
		assert!(!beside(&dmg, "mount").exists(), "unmounted and tidied");
		std::fs::remove_dir_all(dir).ok();
	}

	#[test]
	fn a_build_in_its_build_tree_is_not_replaced() {
		let exe = Path::new("/Users/x/rdm/target/release/Downloads");
		assert!(locate(exe, None, "macos").unwrap_err().contains("build tree"));
	}
}

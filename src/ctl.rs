//! A control socket for debug builds: read the state, and do what a click would do, from a
//! shell -- without the mouse. See spec/workflow.md.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::thread;

use futures::StreamExt;
use futures::channel::{mpsc, oneshot};
use gpui::{App, Context, Entity};
use serde::Serialize;

use crate::app::{Rdm, SortKey, View};
use crate::download::{Download, Filter, Status};
use crate::ui::category_sheet::CategorySheet;
use crate::ui::icon::Icon;

/// Under the build directory, so it is per checkout and gone with `cargo clean`.
pub const SOCKET: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/rdm.sock");

const USAGE: &str = "state | view <detailed|compact|grid> | select <id> | open <id> | settings | fullscreen | \
	pause <id> | resume <id> | remove <id> | filter <label> | status <label|none> | \
	sort <added|name|size|progress|speed|status> [desc] | add <url> | \
	category <name> <icon> <pattern> | preset <name> | categories | edit <id> | extension <id> <ext> <on|off> | icon <id> <name> | color <id> <hex> | custom | advanced | colorhelp | reorder | \
	move <id> <onto id>";

pub fn serve(rdm: Entity<Rdm>, cx: &mut App) {
	let _ = std::fs::remove_file(SOCKET);
	let listener = match UnixListener::bind(SOCKET) {
		Ok(listener) => listener,
		Err(error) => {
			eprintln!("control socket not available at {SOCKET}: {error}");
			return;
		}
	};
	// One line in, one reply out, per connection. The socket threads never touch the app; they
	// hand each line to the foreground and wait for its answer.
	let (tx, mut rx) = mpsc::unbounded::<(String, oneshot::Sender<String>)>();
	thread::spawn(move || {
		for stream in listener.incoming().flatten() {
			let tx = tx.clone();
			thread::spawn(move || {
				let mut line = String::new();
				if BufReader::new(&stream).read_line(&mut line).is_err() {
					return;
				}
				let (reply_tx, reply_rx) = oneshot::channel();
				if tx.unbounded_send((line.trim().to_owned(), reply_tx)).is_ok()
					&& let Ok(reply) = futures::executor::block_on(reply_rx)
				{
					let _ = writeln!(&stream, "{reply}");
				}
			});
		}
	});
	cx.spawn(async move |cx| {
		while let Some((line, reply)) = rx.next().await {
			let answer = rdm.update(cx, |rdm, cx| rdm.command(&line, cx));
			let _ = reply.send(answer);
		}
	})
	.detach();
}

#[derive(Serialize)]
struct CategoryState {
	id: u64,
	name: String,
	icon: &'static str,
	pattern: String,
}

#[derive(Serialize)]
struct State<'a> {
	/// The build as identity.rs knows it: the version, and the run number and commit when made
	/// by the release workflow.
	version: &'static str,
	build: Option<&'static str>,
	commit: Option<&'static str>,
	filter: String,
	categories: Vec<CategoryState>,
	status: Option<&'static str>,
	sort: SortKey,
	ascending: bool,
	view: View,
	selected: Option<u64>,
	/// Downloads with a window open, whether the Settings sheet is up, and which face of the
	/// category sheet is, if any.
	windows: Vec<u64>,
	settings: bool,
	category_sheet: Option<&'static str>,
	downloads: &'a [Download],
}

fn failure(message: &str) -> String {
	serde_json::json!({ "error": message }).to_string()
}

impl Rdm {
	fn state(&self, cx: &mut Context<Self>) -> String {
		let windows = self
			.open
			.iter()
			.filter(|(_, handle)| handle.update(cx, |_, _, _| ()).is_ok())
			.map(|(id, _)| *id)
			.collect();
		let settings = self.settings_open();
		let state = State {
			version: crate::identity::VERSION,
			build: crate::identity::BUILD,
			commit: crate::identity::COMMIT,
			filter: self.filter.label(&self.categories),
			categories: self
				.categories
				.iter()
				.map(|c| CategoryState {
					id: c.id,
					name: c.name.clone(),
					icon: c.icon.name(),
					pattern: c.pattern.clone(),
				})
				.collect(),
			status: self.status.map(Status::label),
			sort: self.sort,
			ascending: self.ascending,
			view: self.view,
			selected: self.selected,
			windows,
			settings,
			category_sheet: self.category_sheet.as_ref().map(|sheet| match sheet {
				CategorySheet::Presets { .. } => "presets",
				CategorySheet::Preset(_) => "preset",
				CategorySheet::Reorder => "reorder",
				CategorySheet::Custom(_) => "custom",
			}),
			downloads: &self.downloads,
		};
		serde_json::to_string_pretty(&state).unwrap_or_else(|error| failure(&error.to_string()))
	}

	/// One line of the protocol above; every command answers with the state it left behind.
	pub(crate) fn command(&mut self, line: &str, cx: &mut Context<Self>) -> String {
		let mut words = line.split_whitespace();
		let verb = words.next().unwrap_or("");
		let rest: Vec<&str> = words.collect();
		let id = rest.first().and_then(|word| word.parse::<u64>().ok());
		let label = rest.join(" ");
		match verb {
			"state" => {}
			"view" => match label.as_str() {
				"detailed" => self.set_view(View::Detailed, cx),
				"compact" => self.set_view(View::Compact, cx),
				"grid" => self.set_view(View::Grid, cx),
				_ => return failure("view takes detailed, compact or grid"),
			},
			"select" | "open" | "pause" | "resume" | "remove" => {
				let Some(id) = id else { return failure(&format!("{verb} takes a download id")) };
				if !self.downloads.iter().any(|d| d.id == id) {
					return failure(&format!("no download {id}"));
				}
				match verb {
					"select" => self.select(id, cx),
					"open" => self.open_download(id, cx),
					"pause" => self.pause(id, cx),
					"resume" => self.resume(id, cx),
					_ => self.remove(id, cx),
				}
			}
			"settings" => self.toggle_settings(!self.settings_open(), cx),
			// The main window is the one whose root is this entity; toggling through it is what
			// the green light does, for looking at the toolbar without the lights.
			"fullscreen" => {
				let this = cx.entity();
				for handle in cx.windows() {
					let _ = handle.update(cx, |root, window, _| {
						if root.entity_id() == this.entity_id() {
							window.toggle_fullscreen();
						}
					});
				}
			}
			"filter" => {
				let states = Filter::STATES.into_iter();
				let categories = self.categories.iter().map(|c| Filter::Category(c.id));
				match states
					.chain(categories)
					.find(|f| f.label(&self.categories).eq_ignore_ascii_case(&label))
				{
					Some(filter) => self.set_filter(filter, cx),
					None => return failure("filter takes a sidebar label"),
				}
			}
			"preset" if !label.is_empty() => self.toggle_preset(&label, cx),
			"categories" => {
				self.category_sheet = Some(CategorySheet::Presets { editing: false });
				cx.notify();
			}
			"reorder" => self.start_reorder(cx),
			"edit" => {
				let Some(id) = id else { return failure("edit takes a preset's category id") };
				self.open_preset_editor(id, None, cx);
			}
			"icon" => {
				let (Some(id), Some(glyph)) = (id, rest.get(1).and_then(|g| Icon::by_name(g))) else {
					return failure("icon takes a category id and one of the icon choices");
				};
				self.set_category_icon(id, glyph, cx);
			}
			"color" => {
				let text = rest[1..].join(" ");
				let Some(id) = id.filter(|_| crate::ui::theme::parse_color(&text).is_some()) else {
					return failure("color takes a category id and a color: hex, rgb() or hsl()");
				};
				self.set_category_custom_color(id, &text, cx);
			}
			"extension" => {
				let (Some(id), Some(extension)) = (id, rest.get(1)) else {
					return failure("extension takes a category id, an extension and on or off");
				};
				let on = rest.get(2) != Some(&"off");
				self.set_preset_extension(id, extension, on, cx);
			}
			"custom" => self.open_custom_form(None, cx),
			"advanced" => self.toggle_advanced(None, cx),
			"colorhelp" => self.show_color_guide(cx),
			"move" => {
				let onto = rest.get(1).and_then(|word| word.parse::<u64>().ok());
				let (Some(id), Some(onto)) = (id, onto) else {
					return failure("move takes a category id and the id of the row to take the place of");
				};
				self.move_category(id, onto, cx);
			}
			"category" => {
				// category <name> <icon> <pattern...>: the name is one word here; the sheet takes any.
				let (Some(name), Some(glyph)) = (rest.first(), rest.get(1)) else {
					return failure("category takes <name> <icon> <pattern>");
				};
				let Some(glyph) = Icon::by_name(glyph) else {
					return failure("icon is one of the category choices");
				};
				let pattern = rest[2..].join(" ");
				if let Err(error) = self.add_category(name, glyph, None, None, &pattern, cx) {
					return failure(&error);
				}
			}
			"status" if label == "none" => {
				self.status = None;
				cx.notify();
			}
			"status" => match Status::ALL.into_iter().find(|s| s.label().eq_ignore_ascii_case(&label)) {
				Some(status) => {
					self.status = Some(status);
					cx.notify();
				}
				None => return failure("status takes a status label, or none"),
			},
			"sort" => {
				let key = match rest.first().copied().unwrap_or("") {
					"added" => SortKey::Added,
					"name" => SortKey::Name,
					"size" => SortKey::Size,
					"progress" => SortKey::Progress,
					"speed" => SortKey::Speed,
					"status" => SortKey::Status,
					_ => return failure("sort takes added, name, size, progress, speed or status"),
				};
				self.sort = key;
				self.ascending = rest.get(1) != Some(&"desc");
				cx.notify();
			}
			"add" if !label.is_empty() => self.add_url(&label, cx),
			"add" => return failure("add takes a url"),
			_ => return failure(USAGE),
		}
		self.state(cx)
	}
}

//! A control socket for debug builds: read the state, and do what a click would do, from a
//! shell -- without the mouse. See spec/workflow.md.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::thread;

use futures::StreamExt;
use futures::channel::{mpsc, oneshot};
use gpui::{App, Context, Entity};
use serde::Serialize;
use serde_json::Value;

use crate::app::{Column, Rdm, SortKey, View};
use crate::download::{Download, Filter, Status};
use crate::ui::category_sheet::CategorySheet;
use crate::ui::icon::Icon;
use crate::ui::text_input::TextInput;

/// Under the build directory, so it is per checkout and gone with `cargo clean`.
pub const SOCKET: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/rdm.sock");

const USAGE: &str = "state | tree | view <detailed|thumbnails|grid> | select <id> | open <id> | settings [section] | menu <label> | fullscreen | update | \
	drag <size|progress|speed|status|added> <points> | say <occasion> [text] | \
	pause <id> | resume <id> | remove <id> | filter <label> | status <label|none> | \
	sort <added|name|size|progress|speed|status> [desc] | add <url> | look <address> | connections <id> <auto|n> | \
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
struct AddState {
	address: String,
	/// The address the engine is looking at, until it answers.
	checking: Option<String>,
	error: Option<String>,
	found: Option<FoundState>,
	page: Option<PageState>,
	more: bool,
	name: String,
	folder: Option<String>,
	checksum: String,
	limit: String,
	range_start: String,
	range_end: String,
}

#[derive(Serialize)]
struct FoundState {
	url: String,
	name: String,
	size: Option<u64>,
	ranges: bool,
	last_modified: Option<String>,
	version: &'static str,
	server: Option<String>,
}

#[derive(Serialize)]
struct PageState {
	url: String,
	links: Vec<String>,
	added: Vec<usize>,
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
	/// The New Task sheet while it is up: what each field holds and what looking at the address
	/// found. See spec/ui.md.
	add: Option<AddState>,
	/// The table, as the header has it: the widths asked for, the widths there is room to draw,
	/// what the name column is left, and how wide the window is. The two rows of widths differ
	/// only when the window is too narrow to hold what was asked for. See spec/ui.md.
	widths: [f32; 5],
	drawn: [f32; 5],
	name_width: f32,
	window_width: f32,
	/// How many rows the list holds and how many it is showing: the funnel's files count in both,
	/// the filters and the status menu cut the second. The list draws only what the window has
	/// room for, so neither is a count of what is on screen -- but a measurement wants to know
	/// what the list was asked to hold. See src/ui/list.rs.
	rows: usize,
	shown: usize,
	downloads: &'a [Download],
}

fn failure(message: &str) -> String {
	serde_json::json!({ "error": message }).to_string()
}

/// One window's dump, reshaped from GPUI's flat map of nodes into the tree it describes, with the
/// frame's counts beside it.
fn nest_window(dump: &Value) -> Value {
	let frame = &dump["frame"];
	let focus = dump["gpui_focus"].as_str();
	let tree = match (dump["root"].as_str(), dump["nodes"].as_object()) {
		(Some(root), Some(nodes)) => nest_node(root, nodes, focus),
		_ => Value::Null,
	};
	serde_json::json!({
		"title": frame["window_title"],
		"nodes": frame["node_count"],
		"tab_stops": frame["tab_stop_count"],
		"viewport": frame["viewport_size"],
		"frame": frame["frame_number"],
		"tree": tree,
	})
}

/// A node's accessibility properties flattened into it, its provenance renamed short, and its
/// children in place of their keys.
fn nest_node(key: &str, nodes: &serde_json::Map<String, Value>, focus: Option<&str>) -> Value {
	let Some(node) = nodes.get(key) else { return Value::Null };
	let mut out = serde_json::Map::new();
	if let Some(aria) = node["aria"].as_object() {
		out.extend(aria.iter().map(|(name, value)| (name.clone(), value.clone())));
	}
	for (from, to) in [("element_id", "id"), ("view", "view"), ("source_location", "at")] {
		if let Some(value) = node.get(from) {
			out.insert(to.to_owned(), value.clone());
		}
	}
	if focus == Some(key) {
		out.insert("focused".to_owned(), Value::Bool(true));
	}
	let children: Vec<Value> = node["children"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.map(|child| nest_node(child, nodes, focus))
		.collect();
	if !children.is_empty() {
		out.insert("children".to_owned(), Value::Array(children));
	}
	Value::Object(out)
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
		let add = self.add_state(cx);
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
			add,
			widths: self.widths,
			drawn: self.drawn(),
			name_width: self.name_width(&self.drawn()),
			window_width: self.viewport.width.into(),
			category_sheet: self.category_sheet.as_ref().map(|sheet| match sheet {
				CategorySheet::Presets { .. } => "presets",
				CategorySheet::Preset(_) => "preset",
				CategorySheet::Reorder => "reorder",
				CategorySheet::Custom(_) => "custom",
			}),
			rows: self.rows().count(),
			shown: self.shown().len(),
			downloads: &self.downloads,
		};
		serde_json::to_string_pretty(&state).unwrap_or_else(|error| failure(&error.to_string()))
	}

	/// The Add Task sheet as its fields hold it, or nothing while it is closed.
	fn add_state(&self, cx: &App) -> Option<AddState> {
		let sheet = self.adding.as_ref()?;
		let read = |field: &Entity<TextInput>| field.read(cx).content.to_string();
		Some(AddState {
			address: read(&sheet.input),
			checking: sheet.checking.as_ref().map(|(url, _)| url.to_string()),
			error: sheet.error.clone(),
			found: sheet.found.as_ref().map(|found| FoundState {
				url: found.url.to_string(),
				name: found.probe.file_name.clone(),
				size: found.probe.size,
				ranges: found.probe.ranges,
				last_modified: found.probe.last_modified.clone(),
				version: found.probe.version,
				server: found.probe.server.clone(),
			}),
			page: sheet.page.as_ref().map(|page| PageState {
				url: page.url.to_string(),
				links: page.links.iter().map(|link| link.url.to_string()).collect(),
				added: page.added.clone(),
			}),
			more: sheet.more,
			name: read(&sheet.name),
			folder: sheet.folder.as_ref().map(|path| path.display().to_string()),
			checksum: read(&sheet.checksum),
			limit: read(&sheet.limit),
			range_start: read(&sheet.range_start),
			range_end: read(&sheet.range_end),
		})
	}

	/// Every window's component tree as GPUI last built it for accessibility, nested, with the
	/// view and source line each node came from. GPUI builds that tree only once something has
	/// asked the window for it, so the reply names the windows still asleep -- a window opened
	/// after the others were woken is one -- and the client wakes them. See spec/workflow.md.
	fn tree(&self, cx: &mut Context<Self>) -> String {
		let mut windows = Vec::new();
		let mut asleep = Vec::new();
		for handle in cx.windows() {
			let _ = handle.update(cx, |_, window, _| {
				let dump = window
					.debug_a11y_tree_json()
					.filter(|_| window.is_a11y_active())
					.and_then(|json| serde_json::from_str::<Value>(&json).ok());
				match dump {
					Some(dump) => windows.push(nest_window(&dump)),
					None => asleep.push(window.window_title()),
				}
			});
		}
		if windows.is_empty() {
			return failure("asleep: no window has built its accessibility tree yet");
		}
		serde_json::to_string_pretty(&serde_json::json!({ "asleep": asleep, "windows": windows }))
			.unwrap_or_else(|error| failure(&error.to_string()))
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
			"tree" => return self.tree(cx),
			"view" => match label.as_str() {
				"detailed" => self.set_view(View::Detailed, cx),
				"thumbnails" => self.set_view(View::Thumbnails, cx),
				"grid" => self.set_view(View::Grid, cx),
				_ => return failure("view takes detailed, thumbnails or grid"),
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
			// Alone, the sheet is toggled; with a section's name, it is opened on that section.
			"settings" if label.is_empty() => self.toggle_settings(!self.settings_open(), cx),
			// Opens or closes one row's dropdown, by any part of its label: a menu is a press
			// away and the pointer is not ours to move. Where it opens is the button's own
			// business, so this says only which row. See spec/workflow.md.
			"menu" => {
				let wanted = label.to_ascii_lowercase();
				let Some(row) = self
					.settings_dropdowns()
					.into_iter()
					.find(|label| crate::i18n::t(label).to_ascii_lowercase().contains(&wanted))
				else {
					return failure("menu takes part of the label of a row that has a dropdown");
				};
				self.toggle_settings_menu(row, cx);
			}
			"settings" => {
				let Some(section) = crate::ui::settings_sheet::Section::ALL
					.into_iter()
					.find(|s| s.name().eq_ignore_ascii_case(&label))
				else {
					return failure("settings takes a section: general, transfers, appearance, about");
				};
				self.open_settings(cx);
				self.set_settings_section(section, cx);
			}
			// Check now, as the settings row does: a hand build is then shown the newest build.
			"update" => self.check_for_updates(true, cx),
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
			// The one gesture the accessibility tree cannot perform, since a handle has no action of
			// its own -- it answers a press and then the pointer, which AXPress is not. The press
			// and every move go through the same three functions a real drag does, so what this
			// exercises is the drag itself and not a copy of it. See spec/workflow.md.
			"drag" => {
				let Some(column) = rest.first().and_then(|name| match *name {
					"size" => Some(Column::Size),
					"progress" => Some(Column::Progress),
					"speed" => Some(Column::Speed),
					"status" => Some(Column::Status),
					"added" => Some(Column::Added),
					_ => None,
				}) else {
					return failure("drag takes size, progress, speed, status or added, then the travel");
				};
				let Some(travel) = rest.get(1).and_then(|by| by.parse::<f32>().ok()) else {
					return failure("drag takes the travel in points, negative to widen the column");
				};
				// Somewhere the pointer could be; only the difference from it is ever read.
				let from = gpui::px(600.0);
				self.begin_resize(column, from);
				// A pointer arrives a step at a time, and a bug that only shows on the second move
				// would hide from a single jump. Ten steps is enough to catch one.
				for step in 1u8..=10 {
					self.resize_to(from + gpui::px(travel * f32::from(step) / 10.0), true, cx);
				}
				self.end_resize(cx);
			}
			// Saying something on demand, which is the only way to see a notice without waiting
			// for a download to finish. Debug builds only, like the rest of this socket.
			"say" => {
				let Some(occasion) = rest.first().and_then(|name| match *name {
					"finished" => Some(crate::notify::Occasion::Finished),
					"failed" => Some(crate::notify::Occasion::Failed),
					"queue" => Some(crate::notify::Occasion::Queue),
					"update" => Some(crate::notify::Occasion::Update),
					_ => None,
				}) else {
					return failure("say takes finished, failed, queue or update");
				};
				// The same words the real call sites use, so what this shows is what ships.
				let text = rest[1..].join(" ");
				let notice = match occasion {
					crate::notify::Occasion::Finished => {
						let mut notice = crate::notify::Notice::new("Download finish", text);
						// A real file where there is one, so the dialog's size and time are the
						// dialog's own rather than a shape drawn around nothing.
						if let Some(finished) = self.downloads.iter().find_map(|d| {
							Some(crate::notify::Finished {
								path: std::path::PathBuf::from(d.path.as_ref()?),
								size: d.size,
								took: (chrono::Local::now() - d.added).to_std().unwrap_or_default(),
							})
						}) {
							notice = notice.about(finished);
						}
						notice
					}
					crate::notify::Occasion::Failed => {
						crate::notify::Notice::new(format!("{text} failed"), "")
					}
					crate::notify::Occasion::Queue => {
						crate::notify::Notice::new("Every download finished", "")
					}
					crate::notify::Occasion::Update => crate::notify::Notice::new(text, ""),
				};
				self.tell_of(occasion, notice, cx);
			}
			// A download's connections, as its window's field sets them: auto, or a count.
			"connections" => {
				let usage = "connections takes a download id and auto or a count";
				let Some(id) = id.filter(|id| self.downloads.iter().any(|d| d.id == *id)) else {
					return failure(usage);
				};
				let Some(text) = rest.get(1) else { return failure(usage) };
				match crate::ui::add_dialog::parse_connections(text) {
					Ok(count) => self.set_task_connections(id, count, cx),
					Err(message) => return failure(&message),
				}
			}
			// Types an address into the open Add Task sheet and looks at it, as Enter would, so the
			// sheet's found and page faces are reachable without the keyboard. What was found before
			// is dropped first: with it in place, Enter is the second step and adds the download.
			"look" if !label.is_empty() => {
				let Some(sheet) = &mut self.adding else {
					return failure("look needs the New Task sheet open: ax press \"Add Task\"");
				};
				sheet.found = None;
				sheet.page = None;
				let input = sheet.input.clone();
				input.update(cx, |input, cx| input.set_content(&label, cx));
				self.submit_add(cx);
			}
			"add" if !label.is_empty() => self.add_url(&label, cx),
			"add" => return failure("add takes a url"),
			_ => return failure(USAGE),
		}
		self.state(cx)
	}
}

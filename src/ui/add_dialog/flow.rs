//! New Task after Continue: what was found, the rules' answer and the mirror question, and every
//! change the second screen's controls make.

use super::*;

impl Rdm {
	/// The download the sheet is about, added with these mirrors, and the sheet closed.
	pub(super) fn download_found(
		&mut self,
		mut asked: crate::app::Asked,
		mirrors: Vec<String>,
		cx: &mut Context<Self>,
	) {
		let Some(sheet) = &self.adding else { return };
		let Some(found) = &sheet.found else { return };
		let typed = sheet.name.read(cx).content.trim().to_owned();
		let name = if typed.is_empty() { found.probe.file_name.clone() } else { typed };
		let url = found.url.clone();
		asked.mirrors = mirrors;
		let id = self.add_request(url, Some(name), None, asked, cx);
		self.close_add(cx);
		// A new download opens its window, as the place to watch it and change it; one taken
		// from a page's links does not, the sheet staying up for the next. See spec/ui.md.
		self.open_download(id, cx);
	}

	/// The mirrors this download goes to as well as its source, or None when the user has to be
	/// asked first: a mirror was found, there is no checksum to hold it to, and nothing was chosen
	/// for this source's domain. A mirror is only ever used with a checksum from the source, or one
	/// the user typed. See spec/rules.md.
	pub(super) fn mirrors_for(
		&self,
		asked: &crate::app::Asked,
		cx: &Context<Self>,
	) -> Option<Vec<String>> {
		let Some(sheet) = &self.adding else { return Some(Vec::new()) };
		// Not answered yet: the download goes from the source, rather than waiting on the rules.
		let Some(resolved) = &sheet.resolved else { return Some(Vec::new()) };
		if resolved.choice == Some(crate::rules::Choice::Never) {
			return Some(Vec::new());
		}
		let strings = |urls: Vec<Url>| urls.into_iter().map(|u| u.to_string()).collect();
		if asked.checksum.is_some() {
			let written = sheet.checksum.read(cx).content.trim().to_owned();
			let typed = sheet.filled_checksum.as_deref() != Some(written.as_str());
			return Some(strings(resolved.usable(&self.rules, typed)));
		}
		if resolved.mirrors.is_empty() || resolved.choice == Some(crate::rules::Choice::Auto) {
			return Some(Vec::new());
		}
		None
	}

	/// One of the third screen's ways out: this download from the source alone, and, with a choice,
	/// that remembered for the source's domain in the custom layer.
	pub(crate) fn decline_mirror(
		&mut self,
		choice: Option<crate::rules::Choice>,
		cx: &mut Context<Self>,
	) {
		if let Some(choice) = choice
			&& let Some(found) = self.adding.as_ref().and_then(|s| s.found.as_ref())
			&& let Some(host) = found.url.host_str()
			&& let Some(paths) = &self.paths
		{
			let domain = crate::rules::domain_of(host);
			match crate::rules::remember(&paths.custom_rules, &domain, choice) {
				Ok(()) => self.rules = std::sync::Arc::new(crate::rules::load(&paths.rule_places())),
				Err(error) => eprintln!("could not remember the choice for {domain}: {error}"),
			}
		}
		let connections = self.preferences.connections;
		match self.asked(connections, cx) {
			Ok(asked) => self.download_found(asked, Vec::new(), cx),
			Err(message) => {
				if let Some(sheet) = &mut self.adding {
					sheet.problem = Some(Problem { summary: message, detail: None });
				}
				cx.notify();
			}
		}
	}

	/// Back from the third screen to the second.
	pub(super) fn stop_asking(&mut self, cx: &mut Context<Self>) {
		if let Some(sheet) = &mut self.adding {
			sheet.asking = false;
			sheet.problem = None;
		}
		cx.notify();
	}

	/// What the second screen's fields ask for, read and checked: each empty when it was left so.
	pub(super) fn asked(
		&self,
		connections: Option<u16>,
		cx: &Context<Self>,
	) -> Result<crate::app::Asked, String> {
		let Some(sheet) = &self.adding else { return Ok(crate::app::Asked::default()) };
		let text = |field: &Entity<TextInput>| field.read(cx).content.trim().to_owned();
		let checksum = text(&sheet.checksum);
		let checksum = if checksum.is_empty() {
			None
		} else {
			crate::engine::Checksum::parse(&checksum)
				.map(|_| checksum)
				.ok_or_else(|| "A checksum is sha256, sha512 or md5, written in hex.".to_owned())
				.map(Some)?
		};
		let size = sheet.found.as_ref().and_then(|found| found.probe.size);
		let range = part_of_file(&text(&sheet.range_start), &text(&sheet.range_end), size)?;
		// A checksum is checked against a whole file; a part of one has nothing to match.
		if range.is_some() && checksum.is_some() {
			return Err("A checksum is for the whole file; clear it to download a part.".to_owned());
		}
		let speed_limit =
			parse_limit(&text(&sheet.limit))?.map(|megabytes| (megabytes * 1_048_576.0) as u64);
		Ok(crate::app::Asked {
			connections,
			directory: sheet.folder.as_ref().map(|p| p.display().to_string()),
			mirrors: Vec::new(),
			checksum,
			range,
			speed_limit,
		})
	}

	/// The file the second screen is about, and the fields a look can fill filled from it: the
	/// name the server gives, and the whole file as the range, so a part is asked for by moving an
	/// end rather than by working out a number.
	pub(super) fn accept(&mut self, found: Found, cx: &mut Context<Self>) {
		if self.adding.is_none() {
			return;
		}
		let name = found.probe.file_name.clone();
		let whole = found.probe.size.filter(|_| found.probe.ranges);
		// What the rules say about it, worked out while the second screen is read: with the settings'
		// proxy and resolver, as the download itself will go.
		let settings = self.preferences.engine_settings(self.proxy_in_use().as_deref());
		let receiver = crate::engine::client::build(&settings, false).ok().map(|client| {
			let job = crate::rules::resolve::resolve(
				self.rules.clone(),
				client,
				found.url.clone(),
				found.probe.clone(),
			);
			self.engine.run(job)
		});
		let Some(sheet) = &mut self.adding else { return };
		sheet.resolving = receiver;
		sheet.resolved = None;
		sheet.asking = false;
		sheet.found = Some(found);
		sheet.confirm = None;
		sheet.problem = None;
		sheet.details = false;
		let (field, start, end) =
			(sheet.name.clone(), sheet.range_start.clone(), sheet.range_end.clone());
		let fill = |field: &Entity<TextInput>, text: &str, cx: &mut Context<Self>| {
			field.update(cx, |input, cx| {
				if input.content.trim().is_empty() {
					input.set_content(text, cx);
				}
			});
		};
		fill(&field, &name, cx);
		if let Some(size) = whole {
			fill(&start, "0", cx);
			fill(&end, &size.to_string(), cx);
		}
		self.follow_limit(cx);
		cx.notify();
	}

	/// Download anyway, for a page, a script or a stylesheet: on to the second screen with it.
	pub(super) fn download_anyway(&mut self, cx: &mut Context<Self>) {
		let Some(confirm) = self.adding.as_mut().and_then(|sheet| sheet.confirm.take()) else { return };
		self.accept(confirm.found, cx);
	}

	/// The arrow before the title: back to the address, forgetting the file and what was filled in
	/// from it. What was typed by hand -- the checksum, the limit, the folder -- stays.
	pub(crate) fn back_to_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(sheet) = &mut self.adding else { return };
		sheet.found = None;
		sheet.confirm = None;
		sheet.problem = None;
		sheet.more = false;
		let input = sheet.input.clone();
		let filled = [sheet.name.clone(), sheet.range_start.clone(), sheet.range_end.clone()];
		for field in filled {
			field.update(cx, |field, cx| field.set_content("", cx));
		}
		window.focus(&input.read(cx).focus(), cx);
		cx.notify();
	}

	/// More options, or fewer: the folder, the limit and the range, shown or put away.
	pub(crate) fn toggle_add_more(&mut self, cx: &mut Context<Self>) {
		if let Some(sheet) = &mut self.adding {
			sheet.more = !sheet.more;
			cx.notify();
		}
	}

	pub(crate) fn toggle_add_details(&mut self, cx: &mut Context<Self>) {
		if let Some(sheet) = &mut self.adding {
			sheet.details = !sheet.details;
			cx.notify();
		}
	}

	/// The low and high ends of the limit slider, in MB/s, as Settings has them.
	pub(crate) fn limit_scale(&self) -> (f64, f64) {
		self.preferences.limit_slider()
	}

	/// The limit field changed, by hand or by the slider: the slider follows it.
	pub(super) fn follow_limit(&mut self, cx: &mut Context<Self>) {
		cx.notify();
		let Some(sheet) = &self.adding else { return };
		let Ok(limit) = parse_limit(&sheet.limit.read(cx).content) else { return };
		// A limit the slider wrote stays where the slider put it: its places are even on the track
		// and the field writes them rounded, and moving the handle to the rounded one would take it
		// off the place the next drag starts from.
		let scale = self.limit_scale();
		if sheet.limit_slider.read(cx).handles().first().is_some_and(|at| limit_at(*at, scale) == limit)
		{
			return;
		}
		let at = limit_position(limit, scale);
		sheet.limit_slider.clone().update(cx, |slider, cx| slider.set(vec![at], cx));
	}

	/// The limit slider moved, or `ctl slide` moved it: the field says the limit it stands for.
	pub(crate) fn slide_limit(&mut self, position: f32, cx: &mut Context<Self>) {
		let Some(sheet) = &self.adding else { return };
		let text = limit_at(position, self.limit_scale()).map(format_limit).unwrap_or_default();
		sheet.limit.clone().update(cx, |input, cx| input.set_content(&text, cx));
	}

	/// A range field changed: the slider's handles follow the two ends.
	pub(super) fn follow_range(&mut self, cx: &mut Context<Self>) {
		cx.notify();
		let Some(sheet) = &self.adding else { return };
		let Some(size) = sheet.found.as_ref().and_then(|f| f.probe.size).filter(|size| *size > 0)
		else {
			return;
		};
		let read = |field: &Entity<TextInput>| field.read(cx).content.trim().parse::<u64>().ok();
		let start = read(&sheet.range_start).unwrap_or(0).min(size);
		let end = read(&sheet.range_end).unwrap_or(size).min(size);
		let along = |bytes: u64| (bytes as f64 / size as f64) as f32;
		let handles = vec![along(start.min(end)), along(end.max(start))];
		sheet.range_slider.clone().update(cx, |slider, cx| slider.set(handles, cx));
	}

	/// A range handle moved: its field says the byte it stands for, a byte short of the other end.
	/// At a drag's level the byte is put back on that level's step, which a position cannot hold
	/// exactly for a file of gigabytes.
	pub(crate) fn slide_range(
		&mut self,
		handle: usize,
		position: f32,
		level: usize,
		cx: &mut Context<Self>,
	) {
		let Some(sheet) = &self.adding else { return };
		let Some(size) = sheet.found.as_ref().and_then(|f| f.probe.size) else { return };
		let read = |field: &Entity<TextInput>| field.read(cx).content.trim().parse::<u64>().ok();
		let (start, end) =
			(read(&sheet.range_start).unwrap_or(0), read(&sheet.range_end).unwrap_or(size));
		let at = part_at(position, level, size);
		let (field, bytes) = if handle == 0 {
			(sheet.range_start.clone(), at.min(end.saturating_sub(1)))
		} else {
			(sheet.range_end.clone(), at.max(start + 1).min(size))
		};
		field.update(cx, |input, cx| input.set_content(&bytes.to_string(), cx));
	}

	/// The system's folder picker, for where the file goes; nothing chosen leaves the download
	/// folder.
	pub(crate) fn choose_add_folder(&mut self, cx: &mut Context<Self>) {
		let receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
			files: false,
			directories: true,
			multiple: false,
			prompt: Some("Save here".into()),
		});
		cx.spawn(async move |this, cx| {
			if let Ok(Ok(Some(paths))) = receiver.await
				&& let Some(path) = paths.into_iter().next()
			{
				let _ = this.update(cx, |this, cx| {
					if let Some(sheet) = &mut this.adding {
						sheet.folder = Some(path);
						cx.notify();
					}
				});
			}
		})
		.detach();
	}

	pub(crate) fn clear_add_folder(&mut self, cx: &mut Context<Self>) {
		if let Some(sheet) = &mut self.adding {
			sheet.folder = None;
			cx.notify();
		}
	}

	/// The engine's answer about the address, if it has arrived. Called by the event pump. A file
	/// moves on to the second screen; a page, a script or a stylesheet is asked about; a failure
	/// stays on the first screen, said in a line.
	pub(crate) fn poll_add(&mut self, cx: &mut Context<Self>) {
		self.poll_rules(cx);
		let Some(sheet) = &mut self.adding else { return };
		let Some((url, receiver)) = &sheet.checking else { return };
		let Ok(answer) = receiver.try_recv() else { return };
		let url = url.clone();
		sheet.checking = None;
		match answer {
			Ok(inspection) => {
				let found = Found { url, probe: inspection.probe };
				match crate::engine::inspect::confirmation(found.probe.content_type.as_deref()) {
					Some(kind) => {
						sheet.confirm =
							Some(Confirm { found, kind, links: inspection.links, added: Vec::new() })
					}
					None => self.accept(found, cx),
				}
			}
			Err(failure) => {
				sheet.problem = Some(Problem { summary: failure.summary, detail: Some(failure.detail) })
			}
		}
		cx.notify();
	}

	/// The rules' answer about the file, if it has arrived: kept for Download, and the checksum it
	/// found written into the checksum field when that is empty, so it is seen and can be cleared.
	pub(super) fn poll_rules(&mut self, cx: &mut Context<Self>) {
		let Some(sheet) = &mut self.adding else { return };
		let Some(receiver) = &sheet.resolving else { return };
		let Ok(resolved) = receiver.try_recv() else { return };
		sheet.resolving = None;
		if let Some((checksum, _)) = &resolved.checksum {
			let text = checksum_text(checksum);
			let field = sheet.checksum.clone();
			if field.read(cx).content.trim().is_empty() {
				field.update(cx, |input, cx| input.set_content(&text, cx));
				if let Some(sheet) = &mut self.adding {
					sheet.filled_checksum = Some(text);
				}
			}
		}
		if let Some(sheet) = &mut self.adding {
			sheet.resolved = Some(resolved);
		}
		cx.notify();
	}

	/// One of the files a page links to. The sheet stays up so several can be taken.
	pub(super) fn add_link(&mut self, index: usize, cx: &mut Context<Self>) {
		let Some(confirm) = self.adding.as_mut().and_then(|s| s.confirm.as_mut()) else { return };
		let Some(link) = confirm.links.get(index).cloned() else { return };
		if confirm.added.contains(&index) {
			return;
		}
		let source = confirm.found.url.to_string();
		confirm.added.push(index);
		let asked =
			crate::app::Asked { connections: self.preferences.connections, ..Default::default() };
		self.add_request(link.url, Some(link.name), Some(source), asked, cx);
	}
}

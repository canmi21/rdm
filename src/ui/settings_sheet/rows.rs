//! Every row of every section, in the order the sections show them.

use super::*;

impl Rdm {
	/// A row for one of the fields: the field while the sheet is up, its value otherwise.
	/// A row whose control is a text field, in a group of its own choosing.
	pub(super) fn field_row(&self, section: Section, key: &'static str) -> Row {
		let (_, _, note) = FIELDS.iter().find(|(k, _, _)| *k == key).copied().unwrap_or((key, "", ""));
		let control = match self.settings.as_ref().and_then(|s| s.fields.get(key)) {
			Some(input) => Control::Field { input: input.clone() },
			None => Control::Value(self.setting_text(key)),
		};
		Row { section, group: "", label: key, title: None, note, control }
	}

	/// Every setting there is, in the rail's order, with what it shows now.
	pub(super) fn settings_rows(&self) -> Vec<Row> {
		let folder = self
			.paths
			.as_ref()
			.map(|p| p.downloads.display().to_string())
			.unwrap_or_else(|| "the working directory".to_owned());
		let mut rows = vec![
			Row {
				section: Section::General,
				group: "settings.group.language",
				label: "settings.label.language",
				title: None,
				note: "settings.note.language",
				control: Control::Choice {
					options: crate::i18n::Language::ALL.iter().map(|l| l.name()).collect(),
					chosen: crate::i18n::Language::ALL
						.iter()
						.position(|l| *l == self.preferences.language)
						.unwrap_or(0),
					set: |this, index, cx| {
						this.set_language(crate::i18n::Language::ALL[index], cx);
					},
				},
			},
			Row {
				section: Section::General,
				group: "settings.group.starting",
				label: "settings.label.start_at_login",
				title: None,
				note: "settings.note.start_at_login",
				control: Control::Switch {
					on: self.preferences.start_at_login,
					set: Rdm::set_start_at_login,
				},
			},
			Row {
				section: Section::General,
				group: "settings.group.where_things_go",
				label: "settings.label.download_folder",
				title: None,
				note: "settings.note.download_folder",
				control: Control::Value(folder),
			},
			Row {
				section: Section::General,
				group: "settings.group.where_things_go",
				note: "settings.note.on_completion",
				label: "settings.label.on_completion",
				title: None,
				control: Control::Value("Do nothing".to_owned()),
			},
			// TODO: a picker once there is a second channel to pick.
			Row {
				section: Section::Updates,
				group: "",
				note: "settings.note.update_channel",
				label: "settings.label.update_channel",
				title: None,
				control: Control::Value(self.preferences.update_channel.name().to_owned()),
			},
			Row {
				section: Section::Updates,
				group: "",
				note: "settings.note.check_for_updates",
				label: "settings.label.check_for_updates",
				title: None,
				control: Control::Switch {
					on: self.preferences.check_updates,
					set: Rdm::set_check_updates,
				},
			},
			Row {
				section: Section::Updates,
				group: "",
				note: "settings.note.check_for_updates",
				label: "settings.label.automatic_updates",
				title: None,
				control: Control::Switch { on: self.preferences.auto_update, set: Rdm::set_auto_update },
			},
			Row {
				section: Section::Updates,
				group: "",
				note: "settings.note.when_found",
				label: "settings.label.when_a_build_is_found",
				title: None,
				control: Control::Choice {
					options: Policy::ALL.iter().map(|p| p.name()).collect(),
					chosen: Policy::ALL
						.iter()
						.position(|p| *p == self.preferences.update_policy)
						.unwrap_or(0),
					set: |this, index, cx| this.set_update_policy(Policy::ALL[index], cx),
				},
			},
			Row {
				section: Section::Updates,
				group: "",
				note: "settings.note.latest_build",
				label: "settings.label.latest_build",
				title: None,
				control: Control::Action {
					word: "Check now",
					note: self.update_status(),
					run: |this, cx| this.check_for_updates(true, cx),
				},
			},
			Row {
				section: Section::Folder,
				group: "settings.group.what_is_listed",
				note: "settings.note.folders",
				label: "settings.label.folders",
				title: None,
				control: Control::Choice {
					options: Folders::ALL.iter().map(|f| f.name()).collect(),
					chosen: Folders::ALL.iter().position(|f| *f == self.preferences.folders).unwrap_or(0),
					set: |this, index, cx| this.set_folders(Folders::ALL[index], cx),
				},
			},
			Row {
				section: Section::Folder,
				group: "settings.group.opening_a_file",
				note: "settings.note.show_with",
				label: "settings.label.show_with",
				title: None,
				control: Control::Value(if cfg!(any(target_os = "macos", windows)) {
					crate::reveal::manager_name().to_owned()
				} else if self.preferences.file_manager.trim().is_empty() {
					"xdg-open".to_owned()
				} else {
					self.preferences.file_manager.clone()
				}),
			},
			Row {
				section: Section::Folder,
				group: "settings.group.what_is_listed",
				note: "settings.note.hide_junk",
				label: "settings.label.hide_junk",
				title: None,
				control: Control::Switch { on: self.preferences.hide_junk, set: Rdm::set_hide_junk },
			},
			Row {
				section: Section::Network,
				group: "settings.group.proxy",
				label: "settings.label.proxy_source",
				title: None,
				note: "settings.note.proxy_source",
				control: Control::Choice {
					options: crate::proxy::Source::ALL.iter().map(|s| s.name()).collect(),
					chosen: crate::proxy::Source::ALL
						.iter()
						.position(|s| *s == self.preferences.proxy_source)
						.unwrap_or(0),
					set: |this, index, cx| this.set_proxy_source(crate::proxy::Source::ALL[index], cx),
				},
			},
			Row {
				section: Section::Network,
				group: "settings.group.what_we_call_ourselves",
				label: "settings.label.user_agent",
				title: None,
				note: "settings.note.user_agent",
				control: Control::Choice {
					options: crate::agent::Agent::offered().iter().map(|a| a.name()).collect(),
					chosen: crate::agent::Agent::offered()
						.iter()
						.position(|a| *a == self.preferences.agent)
						.unwrap_or(0),
					set: |this, index, cx| {
						let chosen = crate::agent::Agent::offered()[index];
						this.set_agent(chosen, cx);
					},
				},
			},
			// The row above chooses a disguise and fills this one, so the two are one setting seen
			// twice: what was picked, and what will actually be sent. They were both called `User
			// agent`, which named the pair rather than either half of it.
			self
				.field_row(Section::Network, "settings.label.user_agent")
				.titled("settings.label.user_agent_sent")
				.under("settings.group.what_we_call_ourselves"),
			self.field_row(Section::Network, "settings.label.proxy").under("settings.group.proxy"),
			Row {
				section: Section::Network,
				group: "settings.group.proxy",
				label: "settings.label.proxy_in_use",
				title: None,
				note: "settings.note.proxy_in_use",
				control: Control::Action {
					word: "Look again",
					note: self.proxy_status(),
					run: |this, cx| this.look_for_proxy(cx),
				},
			},
			self
				.field_row(Section::Transfers, "settings.label.concurrent_downloads")
				.under("settings.group.at_once"),
			Row {
				section: Section::Transfers,
				group: "settings.group.at_once",
				note: "settings.note.make_room",
				label: "settings.label.make_room",
				title: None,
				control: Control::Choice {
					options: vec![
						crate::i18n::t("settings.choice.make_room.newest"),
						crate::i18n::t("settings.choice.make_room.most_left"),
						crate::i18n::t("settings.choice.make_room.slowest"),
					],
					chosen: match self.preferences.bump {
						Bump::Newest => 0,
						Bump::MostLeft => 1,
						Bump::Slowest => 2,
					},
					set: |this, index, cx| {
						this.preferences.bump = [Bump::Newest, Bump::MostLeft, Bump::Slowest][index];
						this.engine.set_bump(this.preferences.bump);
						this.save_config();
						cx.notify();
					},
				},
			},
			self
				.field_row(Section::Transfers, "settings.label.speed_limit")
				.under("settings.group.at_once"),
			self
				.field_row(Section::Transfers, "settings.label.connections")
				.under("settings.group.per_download"),
			self
				.field_row(Section::Transfers, "settings.label.limit_slider_from")
				.under("settings.group.per_download"),
			self
				.field_row(Section::Transfers, "settings.label.limit_slider_to")
				.under("settings.group.per_download"),
			self
				.field_row(Section::Transfers, "settings.label.smallest_segment")
				.under("settings.group.per_download"),
			self
				.field_row(Section::Transfers, "settings.label.connect_timeout")
				.under("settings.group.per_download"),
			self
				.field_row(Section::Transfers, "settings.label.idle_timeout")
				.under("settings.group.per_download"),
			self
				.field_row(Section::Transfers, "settings.label.retries")
				.under("settings.group.per_download"),
			self
				.field_row(Section::Transfers, "settings.label.retry_wait")
				.under("settings.group.per_download"),
			self
				.field_row(Section::Transfers, "settings.label.size_limit")
				.under("settings.group.per_download"),
			Row {
				section: Section::Transfers,
				group: "settings.group.per_download",
				note: "settings.note.http_version",
				label: "settings.label.http_version",
				title: None,
				control: Control::Choice {
					options: vec!["Auto", "HTTP/1.1", "HTTP/2"],
					chosen: match self.preferences.http {
						HttpVersion::Auto => 0,
						HttpVersion::Http1 => 1,
						HttpVersion::Http2 => 2,
					},
					set: |this, index, cx| {
						this.preferences.http =
							[HttpVersion::Auto, HttpVersion::Http1, HttpVersion::Http2][index];
						this.save_config();
						cx.notify();
					},
				},
			},
			self
				.field_row(Section::Transfers, "settings.label.headers")
				.under("settings.group.per_download"),
			self
				.field_row(Section::Transfers, "settings.label.redirects")
				.under("settings.group.per_download"),
			Row {
				section: Section::Transfers,
				group: "settings.group.per_download",
				note: "settings.note.preallocate",
				label: "settings.label.preallocate",
				title: None,
				control: Control::Switch {
					on: self.preferences.preallocate,
					set: |this, on, cx| {
						this.preferences.preallocate = on;
						this.save_config();
						cx.notify();
					},
				},
			},
			Row {
				section: Section::Transfers,
				group: "settings.group.sources",
				note: "settings.note.rules",
				label: "settings.label.rules",
				title: None,
				control: Control::Action {
					word: "Open",
					note: format!(
						"{} rules, {} mirror families",
						self.rules.entries.len(),
						self.rules.families.len()
					),
					run: |this, cx| this.open_rules(cx),
				},
			},
			Row {
				section: Section::Appearance,
				group: "settings.group.the_table",
				note: "settings.note.column_widths",
				label: "settings.label.column_widths",
				title: None,
				control: Control::Action {
					word: "Reset",
					note: String::new(),
					run: |this, cx| this.reset_widths(cx),
				},
			},
			Row {
				section: Section::Appearance,
				group: "settings.group.colors",
				note: "settings.note.colorful",
				label: "settings.label.colorful",
				title: None,
				control: Control::Switch {
					on: self.preferences.colorful_categories,
					set: Rdm::set_colorful_categories,
				},
			},
			Row {
				section: Section::Appearance,
				group: "settings.group.colors",
				note: "settings.note.dim",
				label: "settings.label.dim",
				title: None,
				control: Control::Switch { on: self.preferences.dim_inactive, set: Rdm::set_dim_inactive },
			},
			// What this build is: the name in full lives here, and the numbers that tell one
			// build from another. See spec/release.md.
			Row {
				section: Section::About,
				group: "settings.group.this_build",
				note: "",
				label: "settings.label.application",
				title: None,
				control: Control::Value(identity::NAME.to_owned()),
			},
			Row {
				section: Section::About,
				group: "settings.group.this_build",
				note: "",
				label: "settings.label.version",
				title: None,
				control: Control::Value(match self.updates.this {
					Some(build) => format!("{} ({build})", identity::VERSION),
					None => format!("{}, built by hand", identity::VERSION),
				}),
			},
			Row {
				section: Section::About,
				group: "settings.group.this_build",
				note: "",
				label: "settings.label.commit",
				title: None,
				control: Control::Value(
					identity::COMMIT
						.map(|sha| sha[..sha.len().min(12)].to_owned())
						.unwrap_or_else(|| "none".to_owned()),
				),
			},
			Row {
				section: Section::About,
				group: "",
				note: "settings.note.identifier",
				label: "settings.label.identifier",
				title: None,
				control: Control::Value(identity::id()),
			},
		];
		// One row an occasion, in the order src/notify.rs lists them, so a new occasion is a
		// variant and nothing here.
		rows.extend(Occasion::ALL.map(|occasion| {
			Row {
				section: Section::Notifications,
				group: "settings.group.where_each_is_said",
				note: occasion.note(),
				label: occasion.label(),
				title: None,
				control: Control::Choice {
					options: Style::ALL.iter().map(|style| style.name()).collect(),
					// A style this build no longer offers lands on the first: a row has to light
					// something, and one lighting nothing reads as broken rather than as unset.
					chosen: Style::ALL
						.iter()
						.position(|style| *style == self.preferences.notice(occasion))
						.unwrap_or(0),
					set: notice_setter(occasion),
				},
			}
		}));
		// How names are resolved. The switch that hands the whole business back to the machine
		// comes first, and while it is on the rows under it are not shown: none of them does
		// anything then, and a row that cannot matter is a row read for nothing. See src/dns.rs.
		rows.push(Row {
			section: Section::Network,
			group: "settings.group.names",
			label: "settings.label.dns_force_system",
			title: None,
			note: "settings.note.dns_force_system",
			control: Control::Switch {
				on: self.preferences.dns_force_system,
				set: Rdm::set_dns_force_system,
			},
		});
		if !self.preferences.dns_force_system {
			let transport = self.preferences.dns_transport;
			let offered = crate::dns::Servers::offered(transport);
			rows.push(Row {
				section: Section::Network,
				group: "settings.group.names",
				label: "settings.label.dns_https",
				title: None,
				note: "settings.note.dns_https",
				control: Control::Switch { on: transport.is_https(), set: Rdm::set_dns_https },
			});
			// Only beside the switch above: forcing a transport that is not in use says nothing.
			if transport.is_https() {
				rows.push(Row {
					section: Section::Network,
					group: "settings.group.names",
					label: "settings.label.dns_force_https",
					title: None,
					note: "settings.note.dns_force_https",
					control: Control::Switch {
						on: self.preferences.dns_force_https,
						set: Rdm::set_dns_force_https,
					},
				});
			}
			rows.push(Row {
				section: Section::Network,
				group: "settings.group.names",
				label: "settings.label.dns_servers",
				title: None,
				note: "settings.note.dns_servers",
				control: Control::Choice {
					options: offered.iter().map(|s| s.name(transport)).collect(),
					chosen: offered.iter().position(|s| *s == self.preferences.dns_servers).unwrap_or(0),
					set: |this, index, cx| {
						let transport = this.preferences.dns_transport;
						this.set_dns_servers(crate::dns::Servers::offered(transport)[index], cx);
					},
				},
			});
			// Only where the choice above reads it. Following the machine's own servers means
			// there is nothing to write, and a field that is ignored is worse than no field.
			if self.preferences.dns_servers != crate::dns::Servers::System {
				rows.push(
					self
						.field_row(Section::Network, "settings.label.name_servers")
						.under("settings.group.names"),
				);
			}
			rows.push(
				self
					.field_row(Section::Network, "settings.label.system_domains")
					.under("settings.group.names"),
			);
		}
		rows
	}
}

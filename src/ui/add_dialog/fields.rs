//! What New Task's fields read and write: counts, limits, ranges and the address itself, and where a
//! slider's position lands on each scale.

use super::*;

/// The number in the connections field: one to `Connections::MAX`, or why not.
pub fn parse_count(text: &str) -> Result<u16, String> {
	let max = crate::engine::Connections::MAX;
	match text.trim().parse::<u32>() {
		Ok(n) if (1..=max as u32).contains(&n) => Ok(n as u16),
		_ => Err(format!("Connections must be a number from 1 to {max}.")),
	}
}

/// A connections field as Settings and a download's window read it: empty or `auto` is the
/// engine's own judgement, anything else a count.
pub fn parse_connections(text: &str) -> Result<Option<u16>, String> {
	let text = text.trim();
	if text.is_empty() || text.eq_ignore_ascii_case("auto") {
		Ok(None)
	} else {
		parse_count(text).map(Some)
	}
}

/// The limit field, in megabytes a second: empty or `unlimited` is no limit.
pub fn parse_limit(text: &str) -> Result<Option<f64>, String> {
	let text = text.trim();
	let text = text.strip_suffix("MB/s").or_else(|| text.strip_suffix("mb/s")).unwrap_or(text).trim();
	if text.is_empty() || text.eq_ignore_ascii_case("unlimited") {
		return Ok(None);
	}
	match text.parse::<f64>() {
		Ok(megabytes) if megabytes > 0.0 && megabytes.is_finite() => Ok(Some(megabytes)),
		_ => Err("A limit is a number of MB/s, or empty for none.".to_owned()),
	}
}

/// A limit as the field shows it: to hundredths, with the zeros a round number ends in left off --
/// `37.58`, `12.5`, `40`. Hundredths so two neighbouring places of a fine drag read differently;
/// see spec/ui.md, "A slider reads the hand's intent from its pauses".
pub fn format_limit(megabytes: f64) -> String {
	let text = format!("{megabytes:.2}");
	text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Where along the limit slider a limit sits. The scale runs from the slider's low end to its high
/// end logarithmically, since a megabyte more matters at five and not at ninety; no limit is the
/// far right, past the scale.
pub fn limit_position(limit: Option<f64>, (low, high): (f64, f64)) -> f32 {
	let Some(megabytes) = limit else { return 1.0 };
	let span = (high / low).ln();
	let along = if span > 0.0 { ((megabytes.max(low) / low).ln() / span) as f32 } else { 0.0 };
	along.clamp(0.0, 1.0) * SCALE
}

/// The limit a place on the slider stands for, to hundredths, as the field writes it. None past the
/// middle of the gap after the scale.
pub fn limit_at(position: f32, (low, high): (f64, f64)) -> Option<f64> {
	if position > (1.0 + SCALE) / 2.0 {
		return None;
	}
	let along = f64::from((position / SCALE).clamp(0.0, 1.0));
	let megabytes = low * (high / low).powf(along);
	Some((megabytes * 100.0).round() / 100.0)
}

/// Where a limit slider's position lands at a drag's level: on the level's steps of its log scale --
/// ten across it, a hundred, a thousand -- or on no limit. Five a decade is the
/// preferred numbers -- 1, 1.6, 2.5, 4, 6.3, 10 -- so the coarse places are both evenly spaced on
/// the track and round as the field writes them. See spec/ui.md, "A slider reads the hand's intent
/// from its pauses".
pub fn limit_snap(level: usize, position: f32, (low, high): (f64, f64)) -> f32 {
	if position > (1.0 + SCALE) / 2.0 {
		return 1.0;
	}
	let Some(steps) = crate::ui::slider::steps(level) else { return position };
	let step = SCALE / steps as f32;
	let even = ((position / step).round() * step).min(SCALE);
	if level > 0 {
		return even;
	}
	// The coarsest places are put on the preferred number they stand in for -- 1.6 rather than the
	// 1.58 an even fifth of a decade is -- which is under 1% of a decade away and reads as round.
	let Some(megabytes) = limit_at(even, (low, high)) else { return even };
	let decade = 10f64.powf(megabytes.log10().floor());
	let preferred = [1.0, 1.6, 2.5, 4.0, 6.3, 10.0]
		.map(|m| m * decade)
		.into_iter()
		.min_by(|a, b| (a / megabytes).ln().abs().total_cmp(&(b / megabytes).ln().abs()))
		.unwrap_or(megabytes);
	if (preferred / megabytes).ln().abs() < 0.02 {
		limit_position(Some(preferred), (low, high))
	} else {
		even
	}
}

/// The byte a range handle stands for at a drag's level: on a level's steps it is that many tenths,
/// hundredths or thousandths of the file, worked out in whole numbers, since a position cannot hold
/// a byte of a file of gigabytes exactly. Exact is the position's own byte.
pub fn part_at(position: f32, level: usize, size: u64) -> u64 {
	let position = f64::from(position.clamp(0.0, 1.0));
	match crate::ui::slider::steps(level) {
		Some(n) => {
			let step = (position * f64::from(n)).round() as u128;
			((step * u128::from(size) + u128::from(n) / 2) / u128::from(n)) as u64
		}
		None => (position * size as f64).round() as u64,
	}
}

/// The two range fields as the row keeps them: `start-end` in bytes, the end excluded, or None for
/// the whole file -- both empty, or a start of zero with the end empty or at the file's size.
pub fn part_of_file(start: &str, end: &str, size: Option<u64>) -> Result<Option<String>, String> {
	let number = |text: &str| -> Result<Option<u64>, String> {
		let text = text.trim().replace([' ', ','], "");
		if text.is_empty() {
			Ok(None)
		} else {
			text.parse().map(Some).map_err(|_| "A range is counted in whole bytes.".to_owned())
		}
	};
	let (start, end) = (number(start)?.unwrap_or(0), number(end)?);
	if let Some(size) = size
		&& (start >= size || end.is_some_and(|end| end > size))
	{
		return Err(format!("The file is {size} bytes; the range must lie inside it."));
	}
	if end.is_some_and(|end| end <= start) {
		return Err("A range ends after it starts.".to_owned());
	}
	let whole = start == 0 && end.is_none_or(|end| Some(end) == size);
	Ok((!whole).then(|| match end {
		Some(end) => format!("{start}-{end}"),
		None => format!("{start}-"),
	}))
}

/// Whatever was typed or pasted, as an address if it can be one. With a scheme, it must be
/// http or https. Without one, `example.org/file.zip` is tried as https, which is what the
/// person meant; anything with whitespace or no dot in it is not tried at all.
pub fn parse_address(text: &str) -> Option<Url> {
	let text = text.trim();
	if text.is_empty() || text.len() > CLIPBOARD_LIMIT {
		return None;
	}
	if let Ok(url) = Url::parse(text)
		&& matches!(url.scheme(), "http" | "https")
		&& url.host().is_some()
	{
		return Some(url);
	}
	if text.contains(char::is_whitespace) || !text.contains('.') || text.contains("://") {
		return None;
	}
	Url::parse(&format!("https://{text}")).ok().filter(|u| u.host().is_some())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn an_address_is_read_with_or_without_its_scheme_and_junk_is_not() {
		let ok = |t: &str| parse_address(t).map(|u| u.to_string());
		assert_eq!(ok("https://a.example/x.zip"), Some("https://a.example/x.zip".into()));
		assert_eq!(ok("  http://a.example/x.zip \n"), Some("http://a.example/x.zip".into()));
		assert_eq!(ok("a.example/x.zip"), Some("https://a.example/x.zip".into()));
		assert_eq!(ok("a.example"), Some("https://a.example/".into()));
		assert_eq!(ok("ftp://a.example/x"), None, "not a scheme the engine speaks");
		assert_eq!(ok("hello world"), None);
		assert_eq!(ok("just words"), None);
		assert_eq!(ok("nodot"), None);
		assert_eq!(ok(""), None);
		assert_eq!(ok(&"x".repeat(1001)), None, "over the limit is not looked at");
	}

	#[test]
	fn a_connections_field_is_auto_when_empty_or_said_and_a_count_otherwise() {
		assert_eq!(parse_connections(""), Ok(None));
		assert_eq!(parse_connections(" Auto "), Ok(None));
		assert_eq!(parse_connections("8"), Ok(Some(8)));
		assert!(parse_connections("0").is_err());
		assert!(parse_connections("33").is_err());
		assert!(parse_connections("lots").is_err());
	}

	#[test]
	fn the_range_fields_are_no_range_at_all_until_they_leave_out_part_of_the_file() {
		assert_eq!(part_of_file("", "", Some(5000)), Ok(None));
		assert_eq!(part_of_file("0", "5000", Some(5000)), Ok(None), "the whole file, as prefilled");
		assert_eq!(part_of_file("0", "1000", Some(5000)), Ok(Some("0-1000".into())));
		assert_eq!(part_of_file("1,000", "", Some(5000)), Ok(Some("1000-".into())));
		assert_eq!(part_of_file("", "", None), Ok(None), "a file of unknown size, untouched");
		assert!(part_of_file("0", "6000", Some(5000)).is_err(), "past the end");
		assert!(part_of_file("300", "200", Some(5000)).is_err(), "backwards");
		assert!(part_of_file("a", "", Some(5000)).is_err());
	}

	#[test]
	fn every_level_is_ten_steps_of_what_the_track_shows() {
		let scale = (1.0, 100.0);
		let rate =
			|level, mb: f64| limit_at(limit_snap(level, limit_position(Some(mb), scale), scale), scale);
		let coarse: Vec<Option<f64>> =
			(0..=10).map(|k| limit_at(limit_snap(0, k as f32 * SCALE / 10.0, scale), scale)).collect();
		let preferred = [1.0, 1.6, 2.5, 4.0, 6.3, 10.0, 16.0, 25.0, 40.0, 63.0, 100.0];
		assert_eq!(coarse, preferred.map(Some).to_vec(), "the coarsest is even and reads as round");
		assert_eq!(rate(0, 4.1), Some(4.0));
		assert_eq!(rate(0, 37.0), Some(40.0));
		assert_eq!(limit_snap(0, 0.97, scale), 1.0, "and no limit is a place of its own");
		assert_eq!(rate(1, 37.4), Some(38.02), "then a hundred across, to hundredths");
		assert_eq!(rate(2, 3.44), Some(3.44), "then a thousand");
		let size = 3_888_513_024;
		assert_eq!(part_at(0.3, 0, size), 1_166_553_907, "three tenths of the file, to the byte");
		assert_eq!(part_at(0.31, 1, size), 1_205_439_037, "thirty-one hundredths");
		assert_eq!(part_at(1.0, 0, size), size, "the end is the end");
		assert_eq!(part_at(0.0, 2, size), 0);
	}

	#[test]
	fn the_limit_slider_is_a_log_scale_with_no_limit_past_its_end() {
		let scale = (1.0, 100.0);
		assert_eq!(limit_position(None, scale), 1.0, "no limit is the far right");
		assert_eq!(limit_position(Some(1.0), scale), 0.0);
		assert!(
			(limit_position(Some(10.0), scale) - 0.45).abs() < 1e-4,
			"ten is halfway along the scale"
		);
		assert!((limit_position(Some(100.0), scale) - SCALE).abs() < 1e-6);
		assert_eq!(limit_at(0.0, scale), Some(1.0));
		assert_eq!(limit_at(0.45, scale), Some(10.0));
		assert_eq!(limit_at(SCALE, scale), Some(100.0));
		assert_eq!(limit_at(1.0, scale), None, "the far right is no limit");
		assert_eq!(limit_at(0.2, scale), Some(2.78), "to hundredths");
		assert_eq!(parse_limit(""), Ok(None));
		assert_eq!(parse_limit("5"), Ok(Some(5.0)));
		assert_eq!(parse_limit("2.5 MB/s"), Ok(Some(2.5)));
		assert!(parse_limit("fast").is_err());
		assert_eq!(format_limit(2.8), "2.8");
		assert_eq!(format_limit(40.0), "40");
		assert_eq!(format_limit(37.58), "37.58");
		assert_eq!(format_limit(12.50), "12.5", "a zero at the end is left off");
		assert_eq!(format_limit(3.0), "3", "and so is a point with nothing after it");
	}
}

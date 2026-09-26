//! An HTML file's head, indented as its nesting is: a page as it arrives is often one long line or
//! indented by whatever wrote it, and a card of its first lines should show its shape. Tolerant of
//! a file cut anywhere, since only the head is read: this re-indents tags, it does not parse a
//! document. See spec/ui.md, "A card shows the file".

/// Elements that never hold anything, so open no level.
const VOID: [&str; 16] = [
	"area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
	"track", "wbr", "!doctype", "?xml",
];
/// How long a run of text may be and stay on its element's line.
const SHORT: usize = 48;
/// Elements whose contents are not markup.
const RAW: [&str; 2] = ["script", "style"];

#[derive(Debug, PartialEq)]
enum Token<'a> {
	Open(&'a str, &'a str),
	Close(&'a str, &'a str),
	/// A void element, a self-closed one, a comment or a declaration: a line of its own.
	Alone(&'a str),
	Text(&'a str),
}

/// The name a tag opens with, lowercased: `<Div class=..>` is `div`.
fn name_of(tag: &str) -> String {
	tag
		.trim_start_matches(['<', '/'])
		.split(|c: char| c.is_whitespace() || c == '>' || c == '/')
		.next()
		.unwrap_or_default()
		.to_ascii_lowercase()
}

fn tokens(text: &str) -> Vec<Token<'_>> {
	let mut out = Vec::new();
	let mut rest = text;
	while !rest.is_empty() {
		let Some(at) = rest.find('<') else {
			out.push(Token::Text(rest));
			break;
		};
		if at > 0 {
			out.push(Token::Text(&rest[..at]));
			rest = &rest[at..];
		}
		// A comment runs to its own end, which may hold a `>`.
		let end = if rest.starts_with("<!--") {
			rest.find("-->").map(|e| e + 3)
		} else {
			rest.find('>').map(|e| e + 1)
		};
		let Some(end) = end else {
			// Cut in the middle of a tag: what is there is still worth a line.
			out.push(Token::Alone(rest));
			break;
		};
		let tag = &rest[..end];
		rest = &rest[end..];
		let name = name_of(tag);
		if tag.starts_with("</") {
			out.push(Token::Close(tag, name_str(tag)));
		} else if tag.starts_with("<!") || tag.ends_with("/>") || VOID.contains(&name.as_str()) {
			out.push(Token::Alone(tag));
		} else {
			out.push(Token::Open(tag, name_str(tag)));
			// A script or style holds text to its closing tag, whatever that text looks like.
			if RAW.contains(&name.as_str()) {
				let close = format!("</{name}");
				let stop = rest.to_ascii_lowercase().find(&close).unwrap_or(rest.len());
				out.push(Token::Text(&rest[..stop]));
				rest = &rest[stop..];
			}
		}
	}
	out
}

/// The tag's name as it is written, for comparing an open with its close.
fn name_str(tag: &str) -> &str {
	let inner = tag.trim_start_matches(['<', '/']);
	let end = inner.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(inner.len());
	&inner[..end]
}

/// The first `most` lines of `text` indented two spaces a level. An element holding only a short
/// run of text stays on one line with it, `<title>Page</title>`, which is how a person writes one.
pub fn pretty(text: &str, most: usize) -> Vec<String> {
	let tokens = tokens(text);
	let mut lines = Vec::new();
	let mut depth: usize = 0;
	let mut at = 0;
	let indent = |depth: usize| "  ".repeat(depth);
	while at < tokens.len() && lines.len() < most {
		match &tokens[at] {
			Token::Open(tag, name) => {
				if let (Some(Token::Text(inner)), Some(Token::Close(close, closing))) =
					(tokens.get(at + 1), tokens.get(at + 2))
					&& closing.eq_ignore_ascii_case(name)
					&& !RAW.contains(&name.to_ascii_lowercase().as_str())
					&& collapse(inner).chars().count() <= SHORT
				{
					lines.push(format!("{}{tag}{}{close}", indent(depth), collapse(inner)));
					at += 3;
					continue;
				}
				lines.push(format!("{}{tag}", indent(depth)));
				depth += 1;
			}
			Token::Close(tag, _) => {
				depth = depth.saturating_sub(1);
				lines.push(format!("{}{tag}", indent(depth)));
			}
			Token::Alone(tag) => lines.push(format!("{}{}", indent(depth), collapse(tag))),
			Token::Text(text) => {
				for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
					lines.push(format!("{}{line}", indent(depth)));
				}
			}
		}
		at += 1;
	}
	lines.truncate(most);
	lines
}

fn collapse(text: &str) -> String {
	text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_page_on_one_line_is_indented_by_its_nesting() {
		let page = r#"<!DOCTYPE html><html><head><meta charset="utf-8"><title>Page</title></head><body><div class="a"><p>Hello <b>you</b></p><br/></div></body></html>"#;
		assert_eq!(
			pretty(page, 20),
			[
				"<!DOCTYPE html>",
				"<html>",
				"  <head>",
				"    <meta charset=\"utf-8\">",
				"    <title>Page</title>",
				"  </head>",
				"  <body>",
				"    <div class=\"a\">",
				"      <p>",
				"        Hello",
				"        <b>you</b>",
				"      </p>",
				"      <br/>",
				"    </div>",
				"  </body>",
				"</html>",
			]
		);
	}

	#[test]
	fn a_script_is_kept_as_text_and_a_cut_tag_is_still_a_line() {
		let page = "<script>if (a < b) { go(); }</script><div><a href=\"x";
		assert_eq!(
			pretty(page, 20),
			["<script>", "  if (a < b) { go(); }", "</script>", "<div>", "  <a href=\"x"]
		);
	}
}

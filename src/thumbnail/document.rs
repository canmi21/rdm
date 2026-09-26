//! A document's first blocks -- headings, paragraphs, list items, quotes and code -- for its card in
//! the grid: a Markdown file parsed, a Word or OpenDocument file's paragraphs read out of the XML
//! inside its zip. Only the head of a file is read, and a block cut short is still a block. See
//! spec/ui.md, "A card shows the file".

use std::io::Read;
use std::path::Path;

/// What kind of block, which is what decides how it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
	Heading(u8),
	Paragraph,
	Item,
	Quote,
	Code,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
	pub kind: Kind,
	pub text: String,
}

/// More than a card can hold, so the card is full whatever sizes the blocks come in.
const BLOCKS: usize = 12;
/// How much of a block is kept: past this a card has cut it long since.
const BLOCK_CHARS: usize = 240;
/// How much of a Markdown file is read, and how much of the XML inside a document.
const MARKDOWN_BYTES: usize = 8 * 1024;
const XML_BYTES: u64 = 512 * 1024;

/// Collects blocks, dropping the empty and cutting the long.
#[derive(Default)]
struct Blocks(Vec<Block>);

impl Blocks {
	fn push(&mut self, kind: Kind, text: &str) {
		let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
		if !text.is_empty() && !self.full() {
			self.0.push(Block { kind, text: text.chars().take(BLOCK_CHARS).collect() });
		}
	}

	fn push_code(&mut self, text: &str) {
		for line in text.lines() {
			if self.full() {
				break;
			}
			let line = line.replace('\t', "  ");
			self.0.push(Block { kind: Kind::Code, text: line.chars().take(BLOCK_CHARS).collect() });
		}
	}

	fn full(&self) -> bool {
		self.0.len() >= BLOCKS
	}

	fn done(self) -> Option<Vec<Block>> {
		(!self.0.is_empty()).then_some(self.0)
	}
}

pub fn markdown(path: &Path) -> Option<Vec<Block>> {
	let mut head = vec![0; MARKDOWN_BYTES];
	let read = std::fs::File::open(path).ok()?.read(&mut head).ok()?;
	head.truncate(read);
	parse_markdown(&String::from_utf8_lossy(&head))
}

fn parse_markdown(text: &str) -> Option<Vec<Block>> {
	use pulldown_cmark::{Event, Parser, Tag, TagEnd};
	let mut blocks = Blocks::default();
	let mut kind = Kind::Paragraph;
	let mut quoted = 0;
	let mut listed = 0;
	let mut text_so_far = String::new();
	for event in Parser::new(text) {
		match event {
			Event::Start(Tag::Heading { level, .. }) => kind = Kind::Heading(level as u8),
			Event::Start(Tag::Paragraph) => {
				kind = if listed > 0 {
					Kind::Item
				} else if quoted > 0 {
					Kind::Quote
				} else {
					Kind::Paragraph
				}
			}
			Event::Start(Tag::Item) => {
				listed += 1;
				kind = Kind::Item;
			}
			Event::Start(Tag::BlockQuote(_)) => quoted += 1,
			Event::Start(Tag::CodeBlock(_)) => kind = Kind::Code,
			Event::Text(t) | Event::Code(t) => text_so_far.push_str(&t),
			Event::SoftBreak | Event::HardBreak => {
				text_so_far.push(if kind == Kind::Code { '\n' } else { ' ' })
			}
			Event::End(end) => {
				match end {
					TagEnd::CodeBlock => blocks.push_code(&text_so_far),
					TagEnd::Heading(_) | TagEnd::Paragraph => blocks.push(kind, &text_so_far),
					// A tight list's item holds its text with no paragraph around it.
					TagEnd::Item => {
						blocks.push(Kind::Item, &text_so_far);
						listed -= 1;
					}
					TagEnd::BlockQuote(_) => quoted -= 1,
					_ => continue,
				}
				text_so_far.clear();
				if blocks.full() {
					break;
				}
			}
			_ => {}
		}
	}
	blocks.done()
}

/// A Word file's paragraphs: `word/document.xml`, each `w:p` a block, its style saying whether it
/// is a heading and a numbering property whether it is a list item.
pub fn word(path: &Path) -> Option<Vec<Block>> {
	parse_word(&xml_in_zip(path, "word/document.xml")?)
}

/// An OpenDocument text's: `content.xml`, `text:h` a heading at its outline level and `text:p` a
/// paragraph, an item inside a `text:list-item`.
pub fn open_document(path: &Path) -> Option<Vec<Block>> {
	parse_open_document(&xml_in_zip(path, "content.xml")?)
}

fn xml_in_zip(path: &Path, name: &str) -> Option<String> {
	let mut archive = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
	let entry = archive.by_name(name).ok()?;
	let mut xml = String::new();
	// Lossy on purpose: a file cut at the limit ends mid-character, and the head is still words.
	let mut bytes = Vec::new();
	entry.take(XML_BYTES).read_to_end(&mut bytes).ok()?;
	xml.push_str(&String::from_utf8_lossy(&bytes));
	Some(xml)
}

fn attribute(tag: &quick_xml::events::BytesStart, name: &[u8]) -> Option<String> {
	tag
		.attributes()
		.flatten()
		.find(|a| a.key.as_ref() == name)
		.map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

/// A style id that is a heading: `Title`, or `Heading` and a level, as Word names its built-in
/// ones whatever the language the document was written in.
fn heading_level(style: &str) -> Option<u8> {
	if style.eq_ignore_ascii_case("title") {
		return Some(1);
	}
	let rest = style.strip_prefix("Heading").or_else(|| style.strip_prefix("heading"))?;
	rest.trim().parse::<u8>().ok().map(|level| level.clamp(1, 6))
}

fn parse_word(xml: &str) -> Option<Vec<Block>> {
	use quick_xml::events::Event;
	let mut reader = quick_xml::Reader::from_str(xml);
	let mut blocks = Blocks::default();
	let (mut kind, mut text, mut in_text) = (Kind::Paragraph, String::new(), false);
	loop {
		match reader.read_event() {
			Ok(Event::Start(tag) | Event::Empty(tag)) => match tag.name().as_ref() {
				b"w:p" => (kind, text) = (Kind::Paragraph, String::new()),
				b"w:pStyle" => {
					if let Some(level) = attribute(&tag, b"w:val").as_deref().and_then(heading_level) {
						kind = Kind::Heading(level);
					}
				}
				b"w:numPr" if kind == Kind::Paragraph => kind = Kind::Item,
				b"w:t" => in_text = true,
				b"w:tab" | b"w:br" => text.push(' '),
				_ => {}
			},
			Ok(Event::Text(t)) if in_text => text.push_str(&t.decode().unwrap_or_default()),
			Ok(Event::GeneralRef(r)) if in_text => text.push_str(&entity(&r)),
			Ok(Event::End(tag)) => match tag.name().as_ref() {
				b"w:t" => in_text = false,
				b"w:p" => {
					blocks.push(kind, &text);
					if blocks.full() {
						break;
					}
				}
				_ => {}
			},
			Ok(Event::Eof) | Err(_) => break,
			_ => {}
		}
	}
	blocks.done()
}

fn parse_open_document(xml: &str) -> Option<Vec<Block>> {
	use quick_xml::events::Event;
	let mut reader = quick_xml::Reader::from_str(xml);
	let mut blocks = Blocks::default();
	let (mut kind, mut text, mut depth, mut listed) = (Kind::Paragraph, String::new(), 0, 0);
	loop {
		match reader.read_event() {
			Ok(Event::Start(tag)) => match tag.name().as_ref() {
				b"text:h" => {
					let level =
						attribute(&tag, b"text:outline-level").and_then(|l| l.parse().ok()).unwrap_or(1);
					(kind, text, depth) = (Kind::Heading(level), String::new(), 1);
				}
				b"text:p" if depth == 0 => {
					let item = if listed > 0 { Kind::Item } else { Kind::Paragraph };
					(kind, text, depth) = (item, String::new(), 1);
				}
				b"text:list-item" => listed += 1,
				_ if depth > 0 => depth += 1,
				_ => {}
			},
			Ok(Event::Empty(tag)) if depth > 0 => {
				if matches!(tag.name().as_ref(), b"text:s" | b"text:tab" | b"text:line-break") {
					text.push(' ');
				}
			}
			Ok(Event::Text(t)) if depth > 0 => text.push_str(&t.decode().unwrap_or_default()),
			Ok(Event::GeneralRef(r)) if depth > 0 => text.push_str(&entity(&r)),
			Ok(Event::End(tag)) => {
				if tag.name().as_ref() == b"text:list-item" {
					listed -= 1;
				} else if depth > 0 {
					depth -= 1;
					if depth == 0 {
						blocks.push(kind, &text);
						if blocks.full() {
							break;
						}
					}
				}
			}
			Ok(Event::Eof) | Err(_) => break,
			_ => {}
		}
	}
	blocks.done()
}

/// The five entities XML predefines, and a character reference; anything else is left as written.
fn entity(reference: &quick_xml::events::BytesRef) -> String {
	let name = String::from_utf8_lossy(reference);
	let character = match name.as_ref() {
		"amp" => Some('&'),
		"lt" => Some('<'),
		"gt" => Some('>'),
		"quot" => Some('"'),
		"apos" => Some('\''),
		other => other
			.strip_prefix("#x")
			.and_then(|hex| u32::from_str_radix(hex, 16).ok())
			.or_else(|| other.strip_prefix('#').and_then(|dec| dec.parse().ok()))
			.and_then(char::from_u32),
	};
	character.map_or_else(|| format!("&{name};"), String::from)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn block(kind: Kind, text: &str) -> Block {
		Block { kind, text: text.to_owned() }
	}

	#[test]
	fn markdown_is_read_as_headings_paragraphs_items_and_code() {
		let text = "# Title\n\nSome *words* and `code`.\n\n- one\n- two\n\n> quoted\n\n```\nfn main() {\n\tgo();\n}\n```\n";
		assert_eq!(
			parse_markdown(text).unwrap(),
			[
				block(Kind::Heading(1), "Title"),
				block(Kind::Paragraph, "Some words and code."),
				block(Kind::Item, "one"),
				block(Kind::Item, "two"),
				block(Kind::Quote, "quoted"),
				block(Kind::Code, "fn main() {"),
				block(Kind::Code, "  go();"),
				block(Kind::Code, "}"),
			]
		);
	}

	#[test]
	fn a_word_documents_paragraphs_carry_their_heading_and_list_styles() {
		let xml = r#"<w:document><w:body>
			<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Report</w:t></w:r></w:p>
			<w:p><w:r><w:t xml:space="preserve">Fish &amp; </w:t></w:r><w:r><w:t>chips</w:t></w:r></w:p>
			<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/></w:numPr></w:pPr><w:r><w:t>first</w:t></w:r></w:p>
			<w:p></w:p>
		</w:body></w:document>"#;
		assert_eq!(
			parse_word(xml).unwrap(),
			[
				block(Kind::Heading(1), "Report"),
				block(Kind::Paragraph, "Fish & chips"),
				block(Kind::Item, "first")
			]
		);
	}

	#[test]
	fn an_open_documents_headings_paragraphs_and_items_are_read() {
		let xml = r#"<office:text>
			<text:h text:outline-level="2">Plan</text:h>
			<text:p>One<text:s/><text:span>two</text:span></text:p>
			<text:list><text:list-item><text:p>item</text:p></text:list-item></text:list>
		</office:text>"#;
		assert_eq!(
			parse_open_document(xml).unwrap(),
			[
				block(Kind::Heading(2), "Plan"),
				block(Kind::Paragraph, "One two"),
				block(Kind::Item, "item")
			]
		);
	}
}

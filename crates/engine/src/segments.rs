//! How a file is cut up between connections, and how the cuts move as connections finish.
//! Pure arithmetic over byte ranges, so it is tested without a network, and it is the part of
//! the engine that is written to disk beside a partial file so a download can be resumed.
//! See spec/engine.md.

use serde::{Deserialize, Serialize};

/// A half-open byte range `start..end` of the source. A download of a whole file is the span
/// `0..size`; a download of part of one is whatever the user asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
	pub start: u64,
	pub end: u64,
}

impl Span {
	pub fn new(start: u64, end: u64) -> Span {
		Span { start, end: end.max(start) }
	}

	pub fn len(self) -> u64 {
		self.end - self.start
	}

	pub fn is_empty(self) -> bool {
		self.end == self.start
	}
}

/// One connection's share: a span, and how much of it has landed, counted from its start. A
/// segment is written front to back, so `done` bytes from `start` is exactly what is on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
	pub span: Span,
	pub done: u64,
}

impl Segment {
	pub fn new(span: Span) -> Segment {
		Segment { span, done: 0 }
	}

	/// Where the next byte goes.
	pub fn position(&self) -> u64 {
		self.span.start + self.done
	}

	pub fn remaining(&self) -> u64 {
		self.span.len() - self.done
	}

	pub fn is_complete(&self) -> bool {
		self.done >= self.span.len()
	}
}

/// The whole download as segments. Together they cover the span exactly once; a segment is
/// never removed, only split, so the map only grows and every byte has one owner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
	pub span: Span,
	pub segments: Vec<Segment>,
}

impl Plan {
	/// One segment for the whole span: the plan for a server that cannot serve ranges, for a file
	/// too small to split, and for the first connection of one that will be split later.
	pub fn whole(span: Span) -> Plan {
		Plan { span, segments: vec![Segment::new(span)] }
	}

	/// The span cut into `parts` equal pieces, each at least `min_segment` long; fewer pieces
	/// when the span is too short for that many. What a download starts with when it is allowed
	/// several connections from the outset.
	pub fn split(span: Span, parts: u8, min_segment: u64) -> Plan {
		let min_segment = min_segment.max(1);
		let parts = (u64::from(parts.max(1))).min(span.len() / min_segment).max(1);
		let each = span.len() / parts;
		let segments = (0..parts)
			.map(|i| {
				let start = span.start + i * each;
				let end = if i + 1 == parts { span.end } else { start + each };
				Segment::new(Span::new(start, end))
			})
			.collect();
		Plan { span, segments }
	}

	pub fn done(&self) -> u64 {
		self.segments.iter().map(|s| s.done).sum()
	}

	pub fn remaining(&self) -> u64 {
		self.span.len() - self.done()
	}

	pub fn is_complete(&self) -> bool {
		self.segments.iter().all(Segment::is_complete)
	}

	/// The first segment that has bytes left and no connection on it, given which segments are
	/// being worked. A connection that comes free takes this before it asks for a split.
	pub fn idle(&self, active: &[usize]) -> Option<usize> {
		self
			.segments
			.iter()
			.enumerate()
			.find(|(i, s)| !s.is_complete() && !active.contains(i))
			.map(|(i, _)| i)
	}

	/// aria2's move: when a connection is free and every unfinished segment is taken, the
	/// segment with the most left is cut where its remainder halves, the far half becoming a new
	/// segment for the free connection. Only when the remainder is at least twice `min_segment`,
	/// so neither half is smaller than a segment is allowed to be; None otherwise, and the
	/// connection waits. The near half keeps writing without noticing, since its end simply
	/// moved closer.
	pub fn steal(&mut self, min_segment: u64) -> Option<usize> {
		let min_segment = min_segment.max(1);
		let (index, segment) = self
			.segments
			.iter()
			.enumerate()
			.filter(|(_, s)| s.remaining() >= 2 * min_segment)
			.max_by_key(|(_, s)| s.remaining())?;
		let cut = segment.position() + segment.remaining() / 2;
		let far = Span::new(cut, segment.span.end);
		self.segments[index].span.end = cut;
		self.segments.push(Segment::new(far));
		Some(self.segments.len() - 1)
	}

	/// The segments that are still open, in the order they sit in the file.
	pub fn open(&self) -> impl Iterator<Item = (usize, &Segment)> {
		self.segments.iter().enumerate().filter(|(_, s)| !s.is_complete())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn covers_exactly_once(plan: &Plan) {
		let mut spans: Vec<Span> = plan.segments.iter().map(|s| s.span).collect();
		spans.sort_by_key(|s| s.start);
		assert_eq!(spans.first().map(|s| s.start), Some(plan.span.start));
		assert_eq!(spans.last().map(|s| s.end), Some(plan.span.end));
		for pair in spans.windows(2) {
			assert_eq!(pair[0].end, pair[1].start, "gap or overlap in {spans:?}");
		}
	}

	#[test]
	fn a_split_is_even_and_never_below_the_minimum() {
		let plan = Plan::split(Span::new(0, 100), 4, 10);
		assert_eq!(plan.segments.len(), 4);
		covers_exactly_once(&plan);
		assert_eq!(plan.segments[3].span, Span::new(75, 100));
		let small = Plan::split(Span::new(0, 25), 4, 10);
		assert_eq!(small.segments.len(), 2, "25 bytes hold two segments of ten, not four");
		covers_exactly_once(&small);
		let tiny = Plan::split(Span::new(0, 5), 4, 10);
		assert_eq!(tiny.segments.len(), 1);
		let offset = Plan::split(Span::new(1000, 1100), 2, 1);
		assert_eq!(offset.segments[1].span, Span::new(1050, 1100), "a span need not start at zero");
	}

	#[test]
	fn stealing_halves_the_largest_remainder_and_stops_at_the_minimum() {
		let mut plan = Plan::whole(Span::new(0, 100));
		plan.segments[0].done = 20;
		let new = plan.steal(10).expect("80 left is enough for two of ten");
		assert_eq!(new, 1);
		assert_eq!(plan.segments[0].span, Span::new(0, 60), "cut at 20 + 80 / 2");
		assert_eq!(plan.segments[1].span, Span::new(60, 100));
		covers_exactly_once(&plan);
		assert_eq!(plan.done(), 20, "nothing written was lost in the cut");
		// The larger remainder is always the one cut.
		plan.segments[1].done = 35;
		let new = plan.steal(10).unwrap();
		assert_eq!(plan.segments[new].span, Span::new(40, 60), "segment 0 had 40 left, segment 1 five");
		covers_exactly_once(&plan);
		assert_eq!(plan.steal(30), None, "no remainder holds two of thirty");
	}

	#[test]
	fn idle_finds_open_work_that_nobody_holds() {
		let mut plan = Plan::split(Span::new(0, 30), 3, 1);
		plan.segments[0].done = 10;
		assert_eq!(plan.idle(&[]), Some(1));
		assert_eq!(plan.idle(&[1]), Some(2));
		assert_eq!(plan.idle(&[1, 2]), None);
		assert_eq!(plan.remaining(), 20);
		assert!(!plan.is_complete());
		for s in &mut plan.segments {
			s.done = s.span.len();
		}
		assert!(plan.is_complete());
		assert_eq!(plan.open().count(), 0);
	}

	#[test]
	fn a_plan_survives_the_control_file() {
		let mut plan = Plan::split(Span::new(0, 100), 3, 1);
		plan.segments[1].done = 7;
		let text = serde_json::to_string(&plan).unwrap();
		assert_eq!(serde_json::from_str::<Plan>(&text).unwrap(), plan);
	}
}

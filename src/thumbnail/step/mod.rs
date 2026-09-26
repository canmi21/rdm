//! A STEP file's card: the part drawn solid, as a mesh's card is. A STEP part is surfaces bounded
//! by edges, written out whole -- lines, circles, ellipses and B-splines between two vertices. The
//! edges are parsed by hand and sampled; each face is laid flat in its surface's own parameters,
//! triangulated there and put back on the surface (faces.rs), and the triangles go to the same
//! rasterizer a mesh does. A file whose faces cannot be filled is drawn as its edges instead,
//! stroked with tiny-skia, nearer lines brighter. See spec/ui.md, "A card shows the file".

use std::collections::HashMap;
use std::path::Path;

mod faces;

type Vec3 = [f64; 3];

/// The picture's size, the card face's shape.
const WIDE: u32 = 280;
const HIGH: u32 = 184;
/// How many points a curve is sampled at, a full circle's worth.
const SAMPLES: usize = 48;
/// The most edges drawn; a part past it is thinned.
const MOST_EDGES: usize = 60_000;

/// One value in an entity's parameter list.
#[derive(Debug, Clone, PartialEq)]
enum Value {
	Ref(u64),
	Number(f64),
	List(Vec<Value>),
	/// Strings, enumerations, `$`, `*` and typed values: none of them are read here but a flag.
	Other(String),
}

impl Value {
	fn num(&self) -> Option<f64> {
		match self {
			Value::Number(n) => Some(*n),
			_ => None,
		}
	}

	fn id(&self) -> Option<u64> {
		match self {
			Value::Ref(id) => Some(*id),
			_ => None,
		}
	}

	fn list(&self) -> &[Value] {
		match self {
			Value::List(values) => values,
			_ => &[],
		}
	}
}

/// Every simple entity in the data section: its type and its parameters. A complex entity, several
/// types in one, keeps the parameters of the part that says what it is.
fn entities(text: &str) -> HashMap<u64, (String, Vec<Value>)> {
	let data = text.find("DATA;").map_or(text, |at| &text[at + 5..]);
	let mut out = HashMap::new();
	for statement in data.split(';') {
		let statement = statement.trim();
		let Some(rest) = statement.strip_prefix('#') else { continue };
		let Some((id, body)) = rest.split_once('=') else { continue };
		let Ok(id) = id.trim().parse::<u64>() else { continue };
		let body = body.trim();
		if let Some(complex) = body.strip_prefix('(') {
			// `(BOUNDED_CURVE() B_SPLINE_CURVE(...) B_SPLINE_CURVE_WITH_KNOTS(...) ...)`: the parts
			// read in turn, and a rational B-spline taken as the plain one it carries.
			let mut parts: Vec<(String, Vec<Value>)> = Vec::new();
			let mut chars = complex.char_indices().peekable();
			while let Some(&(start, c)) = chars.peek() {
				if c.is_ascii_alphabetic() {
					let name_end = complex[start..].find('(').map_or(complex.len(), |e| start + e);
					let name = complex[start..name_end].trim().to_owned();
					let (values, used) = parameters(&complex[name_end..]);
					parts.push((name, values));
					let next = name_end + used;
					while chars.peek().is_some_and(|(i, _)| *i < next) {
						chars.next();
					}
				} else {
					chars.next();
				}
			}
			let find = |name: &str| parts.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone());
			if let (Some(curve), Some(knots)) =
				(find("B_SPLINE_CURVE"), find("B_SPLINE_CURVE_WITH_KNOTS"))
			{
				// The curve's degree and points, then the knots, as the simple entity lays them out.
				let mut values = vec![Value::Other(String::new())];
				values.extend(curve);
				values.extend(knots);
				out.insert(id, ("B_SPLINE_CURVE_WITH_KNOTS".to_owned(), values));
			}
			continue;
		}
		let Some(open) = body.find('(') else { continue };
		let name = body[..open].trim().to_owned();
		let (values, _) = parameters(&body[open..]);
		out.insert(id, (name, values));
	}
	out
}

/// A parenthesized parameter list, and how many bytes it took.
fn parameters(text: &str) -> (Vec<Value>, usize) {
	let bytes = text.as_bytes();
	let mut at = 1;
	let mut values = Vec::new();
	while at < bytes.len() {
		match bytes[at] {
			b')' => return (values, at + 1),
			b',' | b' ' | b'\n' | b'\r' | b'\t' => at += 1,
			b'(' => {
				let (inner, used) = parameters(&text[at..]);
				values.push(Value::List(inner));
				at += used;
			}
			b'#' => {
				let end =
					text[at + 1..].find(|c: char| !c.is_ascii_digit()).map_or(text.len(), |e| at + 1 + e);
				values.push(text[at + 1..end].parse().map_or(Value::Other(String::new()), Value::Ref));
				at = end;
			}
			b'\'' => {
				let mut end = at + 1;
				while end < bytes.len() {
					if bytes[end] == b'\'' {
						if bytes.get(end + 1) == Some(&b'\'') {
							end += 2;
							continue;
						}
						break;
					}
					end += 1;
				}
				values.push(Value::Other(String::new()));
				at = end + 1;
			}
			_ => {
				let end = text[at..].find([',', ')']).map_or(text.len(), |e| at + e);
				let word = text[at..end].trim();
				// A typed value, `LENGTH_MEASURE(1.)`, has a list of its own inside it.
				if let Some(open) = word.find('(') {
					let (_, used) = parameters(&text[at + open..]);
					values.push(Value::Other(word[..open].to_owned()));
					at += open + used;
					continue;
				}
				values
					.push(word.parse::<f64>().map_or_else(|_| Value::Other(word.to_owned()), Value::Number));
				at = end;
			}
		}
	}
	(values, at)
}

struct Model {
	entities: HashMap<u64, (String, Vec<Value>)>,
}

impl Model {
	fn get(&self, id: u64, kind: &str) -> Option<&[Value]> {
		self.entities.get(&id).filter(|(name, _)| name == kind).map(|(_, v)| v.as_slice())
	}

	fn point(&self, id: u64) -> Option<Vec3> {
		let values = self.get(id, "CARTESIAN_POINT")?;
		triple(values.get(1)?)
	}

	fn direction(&self, id: u64) -> Option<Vec3> {
		triple(self.get(id, "DIRECTION")?.get(1)?)
	}

	fn vertex(&self, id: u64) -> Option<Vec3> {
		self.point(self.get(id, "VERTEX_POINT")?.get(1)?.id()?)
	}

	/// An axis placement's origin, its axis and its reference direction made square to the axis.
	fn frame(&self, id: u64) -> Option<(Vec3, Vec3, Vec3)> {
		let values = self.get(id, "AXIS2_PLACEMENT_3D")?;
		let origin = self.point(values.get(1)?.id()?)?;
		let axis =
			values.get(2).and_then(Value::id).and_then(|d| self.direction(d)).unwrap_or([0.0, 0.0, 1.0]);
		let axis = unit(axis);
		let reference = values
			.get(3)
			.and_then(Value::id)
			.and_then(|d| self.direction(d))
			.unwrap_or_else(|| if axis[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] });
		let x = unit(sub(reference, scale(axis, dot(reference, axis))));
		Some((origin, axis, x))
	}

	/// An edge as points: its curve between its two vertices.
	fn edge(&self, id: u64) -> Option<Vec<Vec3>> {
		let values = self.get(id, "EDGE_CURVE")?;
		let start = self.vertex(values.get(1)?.id()?)?;
		let end = self.vertex(values.get(2)?.id()?)?;
		let curve = values.get(3)?.id()?;
		let forward = !matches!(values.get(4), Some(Value::Other(flag)) if flag == ".F.");
		let mut points = self.curve(curve, start, end, forward).unwrap_or_else(|| vec![start, end]);
		// A curve drawn against the edge's sense runs end to start; the edge is start to end.
		if points.first().is_some_and(|first| dist(*first, end) < dist(*first, start)) {
			points.reverse();
		}
		Some(points)
	}

	fn curve(&self, id: u64, start: Vec3, end: Vec3, forward: bool) -> Option<Vec<Vec3>> {
		let (name, values) = self.entities.get(&id)?;
		match name.as_str() {
			"LINE" => Some(vec![start, end]),
			"CIRCLE" | "ELLIPSE" => {
				let (origin, axis, x) = self.frame(values.get(1)?.id()?)?;
				let y = cross(axis, x);
				let (a, b) = (
					values.get(2)?.num()?,
					values.get(3).and_then(Value::num).unwrap_or(values.get(2)?.num()?),
				);
				let angle = |p: Vec3| {
					let d = sub(p, origin);
					(dot(d, y) / b).atan2(dot(d, x) / a)
				};
				let (from, to) = if forward { (start, end) } else { (end, start) };
				let begin = angle(from);
				let mut sweep = angle(to) - begin;
				if sweep <= 1e-9 {
					sweep += std::f64::consts::TAU;
				}
				let steps = ((SAMPLES as f64 * sweep / std::f64::consts::TAU).ceil() as usize).max(2);
				Some(
					(0..=steps)
						.map(|i| {
							let t = begin + sweep * i as f64 / steps as f64;
							add(origin, add(scale(x, a * t.cos()), scale(y, b * t.sin())))
						})
						.collect(),
				)
			}
			"B_SPLINE_CURVE_WITH_KNOTS" => {
				let degree = values.get(1)?.num()? as usize;
				let points: Vec<Vec3> =
					values.get(2)?.list().iter().filter_map(|v| self.point(v.id()?)).collect();
				let multiplicities = values.get(6)?.list();
				let knot_values = values.get(7)?.list();
				let mut knots = Vec::new();
				for (m, k) in multiplicities.iter().zip(knot_values) {
					for _ in 0..m.num()? as usize {
						knots.push(k.num()?);
					}
				}
				if points.len() <= degree || knots.len() != points.len() + degree + 1 {
					return None;
				}
				let (low, high) = (knots[degree], knots[points.len()]);
				Some(
					(0..=SAMPLES)
						.map(|i| {
							de_boor(degree, &points, &knots, low + (high - low) * i as f64 / SAMPLES as f64)
						})
						.collect(),
				)
			}
			// A curve lying on a surface keeps its 3D curve first.
			"SURFACE_CURVE" | "SEAM_CURVE" => self.curve(values.get(1)?.id()?, start, end, forward),
			"TRIMMED_CURVE" => self.curve(values.get(1)?.id()?, start, end, forward),
			_ => None,
		}
	}
}

fn de_boor(degree: usize, points: &[Vec3], knots: &[f64], t: f64) -> Vec3 {
	// The span holding `t`, the last one at the curve's end.
	let mut span = degree;
	while span + 1 < points.len() && knots[span + 1] <= t {
		span += 1;
	}
	let mut d: Vec<Vec3> = (0..=degree).map(|j| points[j + span - degree]).collect();
	for r in 1..=degree {
		for j in (r..=degree).rev() {
			let i = j + span - degree;
			let denominator = knots[i + degree + 1 - r] - knots[i];
			let alpha = if denominator.abs() < 1e-12 { 0.0 } else { (t - knots[i]) / denominator };
			d[j] = add(scale(d[j - 1], 1.0 - alpha), scale(d[j], alpha));
		}
	}
	d[degree]
}

fn triple(value: &Value) -> Option<Vec3> {
	let list = value.list();
	Some([list.first()?.num()?, list.get(1)?.num()?, list.get(2).and_then(Value::num).unwrap_or(0.0)])
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
	[a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: Vec3, b: Vec3) -> Vec3 {
	[a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: Vec3, k: f64) -> Vec3 {
	[a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: Vec3, b: Vec3) -> f64 {
	a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: Vec3, b: Vec3) -> Vec3 {
	[a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dist(a: Vec3, b: Vec3) -> f64 {
	let d = sub(a, b);
	dot(d, d).sqrt()
}
fn unit(a: Vec3) -> Vec3 {
	let length = dot(a, a).sqrt();
	if length == 0.0 { a } else { scale(a, 1.0 / length) }
}

/// Every edge of the file, sampled.
fn edges(text: &str) -> Vec<Vec<Vec3>> {
	let model = Model { entities: entities(text) };
	let mut ids: Vec<u64> = model
		.entities
		.iter()
		.filter(|(_, (name, _))| name == "EDGE_CURVE")
		.map(|(id, _)| *id)
		.collect();
	ids.sort_unstable();
	let step = ids.len().div_ceil(MOST_EDGES).max(1);
	ids.into_iter().step_by(step).filter_map(|id| model.edge(id)).collect()
}

/// The part solid, its faces filled as a mesh's card is drawn; its edges as a wireframe where no
/// face could be filled.
pub fn render(path: &Path) -> Option<image::RgbaImage> {
	let text = String::from_utf8_lossy(&std::fs::read(path).ok()?).into_owned();
	let model = Model { entities: entities(&text) };
	let triangles = faces::triangles(&model);
	if !triangles.is_empty() {
		let mesh: Vec<[[f32; 3]; 3]> =
			triangles.iter().map(|t| t.map(|p| p.map(|c| c as f32))).collect();
		return super::model::draw(&mesh);
	}
	draw(&edges(&text))
}

/// The edges from the front right and above, Z up, fitted to the picture: each segment stroked
/// in a pale blue whose strength follows its depth, so the near side of the part reads over the
/// far one without anything being hidden.
fn draw(edges: &[Vec<Vec3>]) -> Option<image::RgbaImage> {
	use resvg::tiny_skia::{Color, LineCap, Paint, PathBuilder, Pixmap, Stroke, Transform};
	let (azimuth, elevation) = (-35f64.to_radians(), 28f64.to_radians());
	let view = |[x, y, z]: Vec3| -> Vec3 {
		let (sa, ca) = azimuth.sin_cos();
		let (se, ce) = elevation.sin_cos();
		let (rx, ry) = (x * ca - y * sa, x * sa + y * ca);
		[rx, z * ce - ry * se, z * se + ry * ce]
	};
	let viewed: Vec<Vec<Vec3>> = edges
		.iter()
		.map(|e| {
			e.iter().copied().filter(|p| p.iter().all(|c| c.is_finite())).map(view).collect::<Vec<_>>()
		})
		.filter(|e| e.len() >= 2)
		.collect();
	let (mut low, mut high) = ([f64::MAX; 3], [f64::MIN; 3]);
	for p in viewed.iter().flatten() {
		for k in 0..3 {
			low[k] = low[k].min(p[k]);
			high[k] = high[k].max(p[k]);
		}
	}
	if viewed.is_empty() || high[0] <= low[0] && high[1] <= low[1] {
		return None;
	}
	let margin = 0.08;
	let span = ((high[0] - low[0]) / (f64::from(WIDE) * (1.0 - 2.0 * margin)))
		.max((high[1] - low[1]) / (f64::from(HIGH) * (1.0 - 2.0 * margin)))
		.max(f64::EPSILON);
	let center = [(low[0] + high[0]) / 2.0, (low[1] + high[1]) / 2.0];
	let depth = (high[2] - low[2]).max(f64::EPSILON);
	let screen = |p: Vec3| {
		(
			((p[0] - center[0]) / span + f64::from(WIDE) / 2.0) as f32,
			(f64::from(HIGH) / 2.0 - (p[1] - center[1]) / span) as f32,
		)
	};
	let mut pixmap = Pixmap::new(WIDE, HIGH)?;
	// Far segments first, so the near ones are stroked over them. Each is split into short runs
	// with one strength each: a long edge crosses the whole depth of the part.
	let mut runs: Vec<(f64, [Vec3; 2])> = Vec::new();
	for edge in &viewed {
		for pair in edge.windows(2) {
			let near = ((pair[0][2] + pair[1][2]) / 2.0 - low[2]) / depth;
			runs.push((near, [pair[0], pair[1]]));
		}
	}
	runs.sort_by(|a, b| a.0.total_cmp(&b.0));
	let stroke = Stroke { width: 0.9, line_cap: LineCap::Round, ..Stroke::default() };
	for (near, [a, b]) in runs {
		let mut path = PathBuilder::new();
		let (ax, ay) = screen(a);
		let (bx, by) = screen(b);
		path.move_to(ax, ay);
		path.line_to(bx, by);
		let Some(path) = path.finish() else { continue };
		let strength = 0.28 + 0.72 * near;
		let mut paint = Paint::default();
		paint.set_color(Color::from_rgba(0.72, 0.82, 0.95, strength as f32)?);
		paint.anti_alias = true;
		pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
	}
	let rgba: Vec<u8> = pixmap
		.pixels()
		.iter()
		.flat_map(|p| {
			let c = p.demultiply();
			[c.red(), c.green(), c.blue(), c.alpha()]
		})
		.collect();
	image::RgbaImage::from_raw(WIDE, HIGH, rgba)
}

#[cfg(test)]
mod tests {
	use super::*;

	const CUBE_EDGE: &str = "DATA;\n#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=CARTESIAN_POINT('',(1.,0.,0.));\n\
		#3=VERTEX_POINT('',#1);\n#4=VERTEX_POINT('',#2);\n#5=DIRECTION('',(1.,0.,0.));\n#6=VECTOR('',#5,1.);\n\
		#7=LINE('',#1,#6);\n#8=EDGE_CURVE('',#3,#4,#7,.T.);\n\
		#9=DIRECTION('',(0.,0.,1.));\n#10=AXIS2_PLACEMENT_3D('',#1,#9,#5);\n#11=CIRCLE('',#10,1.);\n\
		#12=EDGE_CURVE('',#4,#4,#11,.T.);\nENDSEC;";

	#[test]
	fn a_line_is_its_two_vertices_and_a_closed_circle_goes_all_the_way_round() {
		let edges = edges(CUBE_EDGE);
		assert_eq!(edges[0], [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
		let circle = &edges[1];
		assert_eq!(circle.len(), SAMPLES + 1);
		for p in circle {
			assert!((dot(*p, *p).sqrt() - 1.0).abs() < 1e-9, "every point on the circle: {p:?}");
		}
	}

	#[test]
	fn a_b_spline_ends_at_its_end_points() {
		let text = "DATA;\n#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=CARTESIAN_POINT('',(1.,2.,0.));\n\
			#3=CARTESIAN_POINT('',(3.,0.,0.));\n#4=VERTEX_POINT('',#1);\n#5=VERTEX_POINT('',#3);\n\
			#6=B_SPLINE_CURVE_WITH_KNOTS('',2,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.UNSPECIFIED.);\n\
			#7=EDGE_CURVE('',#4,#5,#6,.T.);\nENDSEC;";
		let edge = &edges(text)[0];
		assert_eq!(edge.first(), Some(&[0.0, 0.0, 0.0]));
		assert!(sub(*edge.last().unwrap(), [3.0, 0.0, 0.0]).iter().all(|c| c.abs() < 1e-9));
	}
}

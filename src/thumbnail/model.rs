//! A 3D model's card: the mesh read from STL, OBJ, OFF or 3MF and drawn from above and to the side,
//! flat-shaded, by a small z-buffer rasterizer on the CPU. A z-buffer rather than painting sorted
//! triangles with tiny-skia: a printed part is hundreds of thousands of triangles most smaller than
//! a pixel, and sorting and filling each as a path was the slow way to draw them and the wrong way
//! where two surfaces cross. See spec/ui.md, "A card shows the file".

use std::io::Read;
use std::path::Path;

type Vec3 = [f32; 3];

/// The picture's size: the card face's shape, drawn at twice the size and averaged down, which is
/// the antialiasing.
const WIDE: usize = 280;
const HIGH: usize = 184;
const SUPER: usize = 2;
/// More than any card needs; a mesh past it is thinned by taking every so many triangles.
const MOST_TRIANGLES: usize = 2_000_000;
/// How deep a 3MF's components may nest before the file is taken to be a loop.
const DEEPEST: usize = 8;

/// The model at `path` drawn, or None for a kind not read here or a file that is not one.
pub fn render(path: &Path, extension: &str) -> Option<image::RgbaImage> {
	let triangles = match extension {
		"stl" => stl(&std::fs::read(path).ok()?)?,
		"obj" => obj(&String::from_utf8_lossy(&std::fs::read(path).ok()?)),
		"off" => off(&String::from_utf8_lossy(&std::fs::read(path).ok()?))?,
		"3mf" => three_mf(path)?,
		_ => return None,
	};
	draw(&triangles)
}

fn stl(bytes: &[u8]) -> Option<Vec<[Vec3; 3]>> {
	// A binary file says how many triangles it holds and is exactly that long; a text one starts
	// `solid`, which a binary file's free header may also do, so the length decides first.
	if bytes.len() >= 84 {
		let count = u32::from_le_bytes(bytes[80..84].try_into().ok()?) as usize;
		if bytes.len() == 84 + count * 50 {
			let float = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap_or_default());
			return Some(
				(0..count)
					.map(|i| {
						let at = 84 + i * 50 + 12;
						let v = |k: usize| [float(at + k * 12), float(at + k * 12 + 4), float(at + k * 12 + 8)];
						[v(0), v(1), v(2)]
					})
					.collect(),
			);
		}
	}
	let text = String::from_utf8_lossy(bytes);
	let mut triangles = Vec::new();
	let mut corners = Vec::with_capacity(3);
	for line in text.lines() {
		let mut words = line.split_whitespace();
		if words.next() == Some("vertex") {
			let v: Vec<f32> = words.filter_map(|w| w.parse().ok()).collect();
			if v.len() == 3 {
				corners.push([v[0], v[1], v[2]]);
			}
			if corners.len() == 3 {
				triangles.push([corners[0], corners[1], corners[2]]);
				corners.clear();
			}
		}
	}
	(!triangles.is_empty()).then_some(triangles)
}

/// Y is up in an OBJ, as in most of what writes one; Z is up everywhere else here, so it is turned.
fn obj(text: &str) -> Vec<[Vec3; 3]> {
	let mut vertices: Vec<Vec3> = Vec::new();
	let mut triangles = Vec::new();
	for line in text.lines() {
		let mut words = line.split_whitespace();
		match words.next() {
			Some("v") => {
				let v: Vec<f32> = words.take(3).filter_map(|w| w.parse().ok()).collect();
				if v.len() == 3 {
					vertices.push([v[0], -v[2], v[1]]);
				}
			}
			Some("f") => {
				// `v`, `v/vt`, `v//vn` or `v/vt/vn`, counted from one, or from the end when negative.
				let corners: Vec<Vec3> = words
					.filter_map(|w| w.split('/').next()?.parse::<i64>().ok())
					.filter_map(|i| {
						let at = if i < 0 { vertices.len() as i64 + i } else { i - 1 };
						vertices.get(usize::try_from(at).ok()?).copied()
					})
					.collect();
				// A polygon as a fan from its first corner.
				for k in 1..corners.len().saturating_sub(1) {
					triangles.push([corners[0], corners[k], corners[k + 1]]);
				}
			}
			_ => {}
		}
	}
	triangles
}

fn off(text: &str) -> Option<Vec<[Vec3; 3]>> {
	let mut lines =
		text.lines().map(|l| l.split('#').next().unwrap_or("").trim()).filter(|l| !l.is_empty());
	let first = lines.next()?;
	let counts =
		if first.starts_with("OFF") { first.trim_start_matches("OFF").trim() } else { first };
	let counts = if counts.is_empty() { lines.next()? } else { counts };
	let mut numbers = counts.split_whitespace().filter_map(|w| w.parse::<usize>().ok());
	let (vertex_count, face_count) = (numbers.next()?, numbers.next()?);
	let vertices: Vec<Vec3> = lines
		.by_ref()
		.take(vertex_count)
		.filter_map(|l| {
			let v: Vec<f32> = l.split_whitespace().take(3).filter_map(|w| w.parse().ok()).collect();
			(v.len() == 3).then(|| [v[0], v[1], v[2]])
		})
		.collect();
	let mut triangles = Vec::new();
	for line in lines.take(face_count) {
		let mut words = line.split_whitespace().filter_map(|w| w.parse::<usize>().ok());
		let n = words.next().unwrap_or(0);
		let corners: Vec<Vec3> = words.take(n).filter_map(|i| vertices.get(i).copied()).collect();
		for k in 1..corners.len().saturating_sub(1) {
			triangles.push([corners[0], corners[k], corners[k + 1]]);
		}
	}
	(!triangles.is_empty()).then_some(triangles)
}

/// One `<object>` of a 3MF: its own triangles, and the other objects it places, each with where.
#[derive(Default)]
struct Object {
	triangles: Vec<[Vec3; 3]>,
	components: Vec<(String, String, Option<Affine>)>,
}

/// A 3MF's affine transform, twelve numbers, a point taken as a row times it.
type Affine = [f32; 12];

fn affine(text: &str) -> Option<Affine> {
	let numbers: Vec<f32> = text.split_whitespace().filter_map(|w| w.parse().ok()).collect();
	numbers.try_into().ok()
}

fn apply(m: &Affine, [x, y, z]: Vec3) -> Vec3 {
	[
		x * m[0] + y * m[3] + z * m[6] + m[9],
		x * m[1] + y * m[4] + z * m[7] + m[10],
		x * m[2] + y * m[5] + z * m[8] + m[11],
	]
}

/// A 3MF: a zip of XML models. The main one's build places objects; an object is a mesh, or
/// components placing other objects, which Bambu and Prusa keep in model files of their own.
fn three_mf(path: &Path) -> Option<Vec<[Vec3; 3]>> {
	let mut archive = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
	let names: Vec<String> = archive
		.file_names()
		.filter(|n| n.to_ascii_lowercase().ends_with(".model"))
		.map(str::to_owned)
		.collect();
	let mut objects: std::collections::HashMap<(String, String), Object> = Default::default();
	let mut build: Vec<(String, String, Option<Affine>)> = Vec::new();
	let main = names.iter().find(|n| n.eq_ignore_ascii_case("3D/3dmodel.model")).cloned()?;
	for name in &names {
		let mut xml = String::new();
		archive.by_name(name).ok()?.read_to_string(&mut xml).ok()?;
		let file = format!("/{}", name.trim_start_matches('/'));
		parse_model(&xml, &file, &mut objects, if *name == main { Some(&mut build) } else { None });
	}
	let mut triangles = Vec::new();
	for (file, id, transform) in &build {
		place(&objects, (file, id), transform.as_ref(), 0, &mut triangles);
	}
	(!triangles.is_empty()).then_some(triangles)
}

fn parse_model(
	xml: &str,
	file: &str,
	objects: &mut std::collections::HashMap<(String, String), Object>,
	mut build: Option<&mut Vec<(String, String, Option<Affine>)>>,
) {
	use quick_xml::events::Event;
	let mut reader = quick_xml::Reader::from_str(xml);
	let mut current: Option<(String, Object)> = None;
	let mut vertices: Vec<Vec3> = Vec::new();
	let attribute = |tag: &quick_xml::events::BytesStart, name: &[u8]| {
		tag
			.attributes()
			.flatten()
			.find(|a| a.key.local_name().as_ref() == name)
			.map(|a| String::from_utf8_lossy(&a.value).into_owned())
	};
	loop {
		match reader.read_event() {
			Ok(Event::Start(tag) | Event::Empty(tag)) => match tag.local_name().as_ref() {
				b"object" => {
					current = attribute(&tag, b"id").map(|id| (id, Object::default()));
					vertices.clear();
				}
				b"vertex" => {
					let f = |n: &[u8]| attribute(&tag, n).and_then(|v| v.parse::<f32>().ok());
					if let (Some(x), Some(y), Some(z)) = (f(b"x"), f(b"y"), f(b"z")) {
						vertices.push([x, y, z]);
					}
				}
				b"triangle" => {
					let i = |n: &[u8]| {
						attribute(&tag, n)
							.and_then(|v| v.parse::<usize>().ok())
							.and_then(|i| vertices.get(i).copied())
					};
					if let (Some((_, object)), Some(a), Some(b), Some(c)) =
						(current.as_mut(), i(b"v1"), i(b"v2"), i(b"v3"))
					{
						object.triangles.push([a, b, c]);
					}
				}
				b"component" => {
					if let (Some((_, object)), Some(id)) = (current.as_mut(), attribute(&tag, b"objectid")) {
						let target = attribute(&tag, b"path").unwrap_or_else(|| file.to_owned());
						object.components.push((
							target,
							id,
							attribute(&tag, b"transform").as_deref().and_then(affine),
						));
					}
				}
				b"item" => {
					if let (Some(build), Some(id)) = (build.as_deref_mut(), attribute(&tag, b"objectid")) {
						let target = attribute(&tag, b"path").unwrap_or_else(|| file.to_owned());
						build.push((target, id, attribute(&tag, b"transform").as_deref().and_then(affine)));
					}
				}
				_ => {}
			},
			Ok(Event::End(tag)) if tag.local_name().as_ref() == b"object" => {
				if let Some((id, object)) = current.take() {
					objects.insert((file.to_owned(), id), object);
				}
			}
			Ok(Event::Eof) | Err(_) => break,
			_ => {}
		}
	}
}

fn place(
	objects: &std::collections::HashMap<(String, String), Object>,
	(file, id): (&str, &str),
	transform: Option<&Affine>,
	depth: usize,
	out: &mut Vec<[Vec3; 3]>,
) {
	let Some(object) = objects.get(&(file.to_owned(), id.to_owned())) else { return };
	if depth > DEEPEST {
		return;
	}
	let moved = |v: Vec3| transform.map_or(v, |m| apply(m, v));
	out.extend(object.triangles.iter().map(|t| [moved(t[0]), moved(t[1]), moved(t[2])]));
	for (target, child, inner) in &object.components {
		// The child's own placement first, then this one's.
		let combined = match (inner, transform) {
			(Some(a), Some(b)) => Some(compose(a, b)),
			(Some(a), None) => Some(*a),
			(None, t) => t.copied(),
		};
		place(objects, (target, child), combined.as_ref(), depth + 1, out);
	}
}

/// `a` then `b`, as one transform.
fn compose(a: &Affine, b: &Affine) -> Affine {
	let row = |k: usize| apply(b, [a[k * 3], a[k * 3 + 1], a[k * 3 + 2]]);
	let origin = apply(b, [0.0; 3]);
	let (x, y, z, t) = (row(0), row(1), row(2), apply(b, [a[9], a[10], a[11]]));
	let linear = |v: Vec3| [v[0] - origin[0], v[1] - origin[1], v[2] - origin[2]];
	let (x, y, z) = (linear(x), linear(y), linear(z));
	[x[0], x[1], x[2], y[0], y[1], y[2], z[0], z[1], z[2], t[0], t[1], t[2]]
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
	[a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
	[a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// The triangles seen from the front right and above, Z up, fitted to the picture and lit from the
/// upper left. Both sides of a face are lit, since a mesh's winding is not to be trusted.
pub(super) fn draw(triangles: &[[Vec3; 3]]) -> Option<image::RgbaImage> {
	let step = triangles.len().div_ceil(MOST_TRIANGLES).max(1);
	let (azimuth, elevation) = (-35f32.to_radians(), 28f32.to_radians());
	let view = |[x, y, z]: Vec3| -> Vec3 {
		let (sa, ca) = azimuth.sin_cos();
		let (se, ce) = elevation.sin_cos();
		let (rx, ry) = (x * ca - y * sa, x * sa + y * ca);
		// Screen right, screen up, and toward the viewer.
		[rx, z * ce - ry * se, z * se + ry * ce]
	};
	let viewed: Vec<[Vec3; 3]> = triangles
		.iter()
		.step_by(step)
		.filter(|t| t.iter().flatten().all(|c| c.is_finite()))
		.map(|t| [view(t[0]), view(t[1]), view(t[2])])
		.collect();
	if viewed.is_empty() {
		return None;
	}
	let (mut low, mut high) = ([f32::MAX; 3], [f32::MIN; 3]);
	for corner in viewed.iter().flatten() {
		for k in 0..3 {
			low[k] = low[k].min(corner[k]);
			high[k] = high[k].max(corner[k]);
		}
	}
	let (w, h) = (WIDE * SUPER, HIGH * SUPER);
	let margin = 0.08;
	let span = ((high[0] - low[0]) / (w as f32 * (1.0 - 2.0 * margin)))
		.max((high[1] - low[1]) / (h as f32 * (1.0 - 2.0 * margin)))
		.max(f32::EPSILON);
	let center = [(low[0] + high[0]) / 2.0, (low[1] + high[1]) / 2.0];
	let screen = |v: Vec3| -> Vec3 {
		[(v[0] - center[0]) / span + w as f32 / 2.0, h as f32 / 2.0 - (v[1] - center[1]) / span, v[2]]
	};
	let light = {
		let l = [-0.45f32, 0.6, 0.66];
		let n = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
		[l[0] / n, l[1] / n, l[2] / n]
	};
	let mut depth = vec![f32::MIN; w * h];
	let mut shade = vec![0f32; w * h];
	for t in &viewed {
		let normal = cross(sub(t[1], t[0]), sub(t[2], t[0]));
		let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
		if length == 0.0 {
			continue;
		}
		let lit = ((normal[0] * light[0] + normal[1] * light[1] + normal[2] * light[2]) / length).abs();
		let brightness = 0.28 + 0.72 * lit;
		let [a, b, c] = [screen(t[0]), screen(t[1]), screen(t[2])];
		let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
		if area.abs() < f32::EPSILON {
			continue;
		}
		let x0 = a[0].min(b[0]).min(c[0]).floor().max(0.0) as usize;
		let x1 = (a[0].max(b[0]).max(c[0]).ceil() as usize).min(w - 1);
		let y0 = a[1].min(b[1]).min(c[1]).floor().max(0.0) as usize;
		let y1 = (a[1].max(b[1]).max(c[1]).ceil() as usize).min(h - 1);
		for y in y0..=y1 {
			let py = y as f32 + 0.5;
			for x in x0..=x1 {
				let px = x as f32 + 0.5;
				let wa = ((b[0] - px) * (c[1] - py) - (b[1] - py) * (c[0] - px)) / area;
				let wb = ((c[0] - px) * (a[1] - py) - (c[1] - py) * (a[0] - px)) / area;
				let wc = 1.0 - wa - wb;
				if wa < 0.0 || wb < 0.0 || wc < 0.0 {
					continue;
				}
				let z = wa * a[2] + wb * b[2] + wc * c[2];
				let at = y * w + x;
				if z > depth[at] {
					depth[at] = z;
					shade[at] = brightness;
				}
			}
		}
	}
	// A cool grey-blue, the color of the resin a render is usually shown in; averaged down, and
	// transparent where nothing was drawn so the card shows through.
	let base = [0.70f32, 0.78, 0.88];
	let mut picture = image::RgbaImage::new(WIDE as u32, HIGH as u32);
	for (x, y, pixel) in picture.enumerate_pixels_mut() {
		let (mut color, mut covered) = ([0f32; 3], 0f32);
		for sy in 0..SUPER {
			for sx in 0..SUPER {
				let at = (y as usize * SUPER + sy) * w + x as usize * SUPER + sx;
				if depth[at] > f32::MIN {
					covered += 1.0;
					for k in 0..3 {
						color[k] += base[k] * shade[at];
					}
				}
			}
		}
		if covered > 0.0 {
			let alpha = covered / (SUPER * SUPER) as f32;
			let channel = |k: usize| (color[k] / covered * 255.0).clamp(0.0, 255.0) as u8;
			*pixel = image::Rgba([channel(0), channel(1), channel(2), (alpha * 255.0) as u8]);
		}
	}
	Some(picture)
}

#[cfg(test)]
mod tests {
	use super::*;

	const CUBE_OBJ: &str = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\n\
		f 1 2 3 4\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n";

	#[test]
	fn an_obj_polygon_is_a_fan_of_triangles() {
		assert_eq!(obj(CUBE_OBJ).len(), 12);
	}

	#[test]
	fn a_binary_stl_is_told_from_a_text_one_by_its_length() {
		let mut bytes = b"solid pretending".to_vec();
		bytes.resize(80, 0);
		bytes.extend(1u32.to_le_bytes());
		bytes.extend([0u8; 12]);
		for v in [[0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
			for c in v {
				bytes.extend(c.to_le_bytes());
			}
		}
		bytes.extend([0u8; 2]);
		assert_eq!(stl(&bytes).unwrap(), [[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]]);
		let text = "solid a\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\n";
		assert_eq!(stl(text.as_bytes()).unwrap().len(), 1);
	}

	#[test]
	fn an_off_file_is_read_to_triangles() {
		let text = "OFF\n4 1 0\n0 0 0\n1 0 0\n1 1 0\n0 1 0\n4 0 1 2 3\n";
		assert_eq!(off(text).unwrap().len(), 2);
	}

	#[test]
	fn a_3mf_component_is_placed_by_both_transforms() {
		let mut objects = Default::default();
		let mut build = Vec::new();
		let xml = r#"<model><resources>
			<object id="1"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
			<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object>
			<object id="2"><components><component objectid="1" transform="1 0 0 0 1 0 0 0 1 10 0 0"/></components></object>
			</resources><build><item objectid="2" transform="1 0 0 0 1 0 0 0 1 0 5 0"/></build></model>"#;
		parse_model(xml, "/3D/3dmodel.model", &mut objects, Some(&mut build));
		let mut triangles = Vec::new();
		for (file, id, transform) in &build {
			place(&objects, (file, id), transform.as_ref(), 0, &mut triangles);
		}
		assert_eq!(triangles, [[[10.0, 5.0, 0.0], [11.0, 5.0, 0.0], [10.0, 6.0, 0.0]]]);
	}

	#[test]
	fn a_cube_draws_shaded_and_leaves_the_corners_clear() {
		let picture = draw(&obj(CUBE_OBJ)).unwrap();
		assert_eq!((picture.width(), picture.height()), (WIDE as u32, HIGH as u32));
		assert_eq!(picture.get_pixel(0, 0)[3], 0, "nothing drawn at the corner");
		let middle = picture.get_pixel(WIDE as u32 / 2, HIGH as u32 / 2);
		assert_eq!(middle[3], 255, "the cube covers the middle");
	}
}

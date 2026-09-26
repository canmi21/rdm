//! A STEP part's faces as triangles. Each face is its surface bounded by loops of edges; the loops
//! are laid flat in the surface's own parameters -- a plane's two axes, a cylinder's or cone's angle
//! and height, a sphere's or torus's two angles -- triangulated there with earcut, subdivided, and
//! each corner put back on the surface, so a cylinder is round and not a fan of chords. A surface
//! with no parameters worked out here, a B-spline patch among them, is laid flat on the plane that
//! fits its loops best and drawn as that flat patch. A face that cannot be laid flat is passed
//! over; the card is a glance, not a measurement.

use super::{Model, Value, Vec3, add, cross, dot, scale, sub, unit};

use std::f64::consts::{PI, TAU};

/// The most a parameter triangle's edge may turn, in radians, before it is split: fine enough that
/// a cylinder reads round at a card's size.
const TURN: f64 = 0.3;
/// How many times a triangle is split at most.
const DEPTH: usize = 4;
/// The most faces filled; a part past it is thinned.
const MOST_FACES: usize = 20_000;

/// How a surface's parameters map to space and back.
enum Surface {
	/// Origin, two axes in the plane.
	Plane {
		origin: Vec3,
		x: Vec3,
		y: Vec3,
	},
	/// A cone of half-angle `slope` is a cylinder whose radius grows with height; a cylinder's is 0.
	Cone {
		origin: Vec3,
		axis: Vec3,
		x: Vec3,
		y: Vec3,
		radius: f64,
		slope: f64,
	},
	Sphere {
		origin: Vec3,
		axis: Vec3,
		x: Vec3,
		y: Vec3,
		radius: f64,
	},
	Torus {
		origin: Vec3,
		axis: Vec3,
		x: Vec3,
		y: Vec3,
		major: f64,
		minor: f64,
	},
}

impl Surface {
	fn of(model: &Model, id: u64) -> Option<Surface> {
		let (name, values) = model.entities.get(&id)?;
		let frame = || model.frame(values.get(1)?.id()?);
		let number = |at: usize| values.get(at).and_then(Value::num);
		Some(match name.as_str() {
			"PLANE" => {
				let (origin, axis, x) = frame()?;
				Surface::Plane { origin, x, y: cross(axis, x) }
			}
			"CYLINDRICAL_SURFACE" => {
				let (origin, axis, x) = frame()?;
				Surface::Cone { origin, axis, x, y: cross(axis, x), radius: number(2)?, slope: 0.0 }
			}
			"CONICAL_SURFACE" => {
				let (origin, axis, x) = frame()?;
				Surface::Cone {
					origin,
					axis,
					x,
					y: cross(axis, x),
					radius: number(2)?,
					slope: number(3)?.tan(),
				}
			}
			"SPHERICAL_SURFACE" => {
				let (origin, axis, x) = frame()?;
				Surface::Sphere { origin, axis, x, y: cross(axis, x), radius: number(2)? }
			}
			"TOROIDAL_SURFACE" => {
				let (origin, axis, x) = frame()?;
				Surface::Torus { origin, axis, x, y: cross(axis, x), major: number(2)?, minor: number(3)? }
			}
			_ => return None,
		})
	}

	/// Which of the two parameters are angles, and so wrap.
	fn wraps(&self) -> [bool; 2] {
		match self {
			Surface::Plane { .. } => [false, false],
			Surface::Cone { .. } => [true, false],
			Surface::Sphere { .. } | Surface::Torus { .. } => [true, true],
		}
	}

	fn flatten(&self, p: Vec3) -> [f64; 2] {
		match self {
			Surface::Plane { origin, x, y } => {
				let d = sub(p, *origin);
				[dot(d, *x), dot(d, *y)]
			}
			Surface::Cone { origin, axis, x, y, .. } => {
				let d = sub(p, *origin);
				[dot(d, *y).atan2(dot(d, *x)), dot(d, *axis)]
			}
			Surface::Sphere { origin, axis, x, y, .. } => {
				let d = sub(p, *origin);
				let across = (dot(d, *x).powi(2) + dot(d, *y).powi(2)).sqrt();
				[dot(d, *y).atan2(dot(d, *x)), dot(d, *axis).atan2(across)]
			}
			Surface::Torus { origin, axis, x, y, major, .. } => {
				let d = sub(p, *origin);
				let across = (dot(d, *x).powi(2) + dot(d, *y).powi(2)).sqrt();
				[dot(d, *y).atan2(dot(d, *x)), dot(d, *axis).atan2(across - major)]
			}
		}
	}

	fn place(&self, [u, v]: [f64; 2]) -> Vec3 {
		let around = |x: Vec3, y: Vec3| add(scale(x, u.cos()), scale(y, u.sin()));
		match self {
			Surface::Plane { origin, x, y } => add(*origin, add(scale(*x, u), scale(*y, v))),
			Surface::Cone { origin, axis, x, y, radius, slope } => {
				add(*origin, add(scale(around(*x, *y), radius + v * slope), scale(*axis, v)))
			}
			Surface::Sphere { origin, axis, x, y, radius } => {
				add(*origin, add(scale(around(*x, *y), radius * v.cos()), scale(*axis, radius * v.sin())))
			}
			Surface::Torus { origin, axis, x, y, major, minor } => add(
				*origin,
				add(scale(around(*x, *y), major + minor * v.cos()), scale(*axis, minor * v.sin())),
			),
		}
	}
}

/// A face's loops as points, the outer first.
fn loops(model: &Model, bounds: &[Value]) -> Vec<Vec<Vec3>> {
	let mut outer = Vec::new();
	let mut inner = Vec::new();
	for bound in bounds {
		let Some(id) = bound.id() else { continue };
		let Some((kind, values)) = model.entities.get(&id) else { continue };
		let Some(loop_id) = values.get(1).and_then(Value::id) else { continue };
		let Some(edges) = model.get(loop_id, "EDGE_LOOP").and_then(|v| v.get(1)) else { continue };
		let mut points: Vec<Vec3> = Vec::new();
		for oriented in edges.list() {
			let Some(values) = oriented.id().and_then(|id| model.get(id, "ORIENTED_EDGE")) else {
				continue;
			};
			let Some(mut edge) = values.get(3).and_then(Value::id).and_then(|id| model.edge(id)) else {
				continue;
			};
			if matches!(values.get(4), Some(Value::Other(flag)) if flag == ".F.") {
				edge.reverse();
			}
			// Each edge starts where the last ended; the shared point is kept once.
			let skip = usize::from(!points.is_empty());
			points.extend(edge.into_iter().skip(skip));
		}
		if points.len() > 1 && super::dist(points[0], *points.last().unwrap_or(&points[0])) < 1e-9 {
			points.pop();
		}
		if points.len() >= 2 {
			if kind == "FACE_OUTER_BOUND" { outer.push(points) } else { inner.push(points) }
		}
	}
	outer.extend(inner);
	outer
}

/// Angles made continuous along a loop, so a loop around a cylinder runs a whole turn rather than
/// jumping back at the seam.
fn unwrap(points: &mut [[f64; 2]], k: usize) {
	for i in 1..points.len() {
		let step = points[i][k] - points[i - 1][k];
		points[i][k] -= TAU * (step / TAU).round();
	}
}

/// A ring's area in the plane, by the shoelace, without its sign.
fn area(ring: &[[f64; 2]]) -> f64 {
	let mut twice = 0.0;
	for (i, a) in ring.iter().enumerate() {
		let b = ring[(i + 1) % ring.len()];
		twice += a[0] * b[1] - b[0] * a[1];
	}
	(twice / 2.0).abs()
}

/// The ring that encloses the most first, as the outline: not every file marks its outer bound,
/// and a hole taken for the outline fills the hole and leaves the face empty.
fn outline_first(rings: &mut [Vec<[f64; 2]>]) {
	if let Some(widest) =
		(0..rings.len()).max_by(|a, b| area(&rings[*a]).total_cmp(&area(&rings[*b])))
	{
		rings.swap(0, widest);
	}
}

fn span(points: &[[f64; 2]], k: usize) -> (f64, f64) {
	points.iter().fold((f64::MAX, f64::MIN), |(low, high), p| (low.min(p[k]), high.max(p[k])))
}

/// Every face of the part as triangles in space.
pub fn triangles(model: &Model) -> Vec<[Vec3; 3]> {
	let mut ids: Vec<u64> = model
		.entities
		.iter()
		.filter(|(_, (name, _))| name == "ADVANCED_FACE" || name == "FACE_SURFACE")
		.map(|(id, _)| *id)
		.collect();
	ids.sort_unstable();
	let step = ids.len().div_ceil(MOST_FACES).max(1);
	let mut out = Vec::new();
	for id in ids.into_iter().step_by(step) {
		let Some((_, values)) = model.entities.get(&id) else { continue };
		let (Some(bounds), Some(surface)) = (values.get(1), values.get(2).and_then(Value::id)) else {
			continue;
		};
		let rings = loops(model, bounds.list());
		if rings.is_empty() {
			continue;
		}
		match Surface::of(model, surface) {
			Some(surface) => face(&surface, &rings, &mut out),
			None => flat(&rings, &mut out),
		}
	}
	out
}

/// A face on a surface this file can lay flat.
fn face(surface: &Surface, rings: &[Vec<Vec3>], out: &mut Vec<[Vec3; 3]>) {
	let wraps = surface.wraps();
	let mut flat: Vec<Vec<[f64; 2]>> =
		rings.iter().map(|r| r.iter().map(|p| surface.flatten(*p)).collect()).collect();
	for ring in &mut flat {
		for (k, wrap) in wraps.iter().enumerate() {
			if *wrap {
				unwrap(ring, k);
			}
		}
	}
	// A band round a cylinder, bounded by two circles and no seam: each circle lays flat as a line,
	// with nothing between them to triangulate, so the band is a strip from one to the next.
	if wraps[0]
		&& flat.len() >= 2
		&& flat.iter().all(|r| {
			let (low, high) = span(r, 0);
			high - low > 1.9 * PI
		}) {
		let mut bands: Vec<(f64, f64)> =
			flat.iter().map(|r| span(r, 1)).map(|(l, h)| (l + h) / 2.0).map(|v| (v, v)).collect();
		bands.sort_by(|a, b| a.0.total_cmp(&b.0));
		for pair in bands.windows(2) {
			let (v0, v1) = (pair[0].0, pair[1].0);
			let steps = 48;
			for i in 0..steps {
				let (u0, u1) = (TAU * i as f64 / steps as f64, TAU * (i + 1) as f64 / steps as f64);
				let corner = |u: f64, v: f64| surface.place([u, v]);
				out.push([corner(u0, v0), corner(u1, v0), corner(u1, v1)]);
				out.push([corner(u0, v0), corner(u1, v1), corner(u0, v1)]);
			}
		}
		return;
	}
	outline_first(&mut flat);
	// A hole that came out a turn away from the outer loop is brought back beside it.
	if let Some(outer) = flat.first().cloned() {
		for ring in flat.iter_mut().skip(1) {
			for (k, wrap) in wraps.iter().enumerate() {
				if !*wrap {
					continue;
				}
				let (low, high) = span(&outer, k);
				let (l, h) = span(ring, k);
				let shift = TAU * (((low + high) / 2.0 - (l + h) / 2.0) / TAU).round();
				for p in ring.iter_mut() {
					p[k] += shift;
				}
			}
		}
	}
	for [a, b, c] in cut(&flat) {
		split(surface, wraps, [a, b, c], 0, out);
	}
}

/// A triangle in parameters, split while any edge turns further than `TURN`, then put on the surface.
fn split(
	surface: &Surface,
	wraps: [bool; 2],
	t: [[f64; 2]; 3],
	depth: usize,
	out: &mut Vec<[Vec3; 3]>,
) {
	let turn = |a: [f64; 2], b: [f64; 2]| {
		(0..2).filter(|k| wraps[*k]).map(|k| (a[k] - b[k]).abs()).fold(0.0, f64::max)
	};
	let widest = turn(t[0], t[1]).max(turn(t[1], t[2])).max(turn(t[2], t[0]));
	if depth >= DEPTH || widest <= TURN {
		out.push(t.map(|p| surface.place(p)));
		return;
	}
	let mid = |a: [f64; 2], b: [f64; 2]| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
	let (ab, bc, ca) = (mid(t[0], t[1]), mid(t[1], t[2]), mid(t[2], t[0]));
	for piece in [[t[0], ab, ca], [ab, t[1], bc], [ca, bc, t[2]], [ab, bc, ca]] {
		split(surface, wraps, piece, depth + 1, out);
	}
}

/// Rings in the plane, the first the outline and the rest holes, as triangles.
fn cut(rings: &[Vec<[f64; 2]>]) -> Vec<[[f64; 2]; 3]> {
	let mut points: Vec<[f64; 2]> = Vec::new();
	let mut holes: Vec<u32> = Vec::new();
	for (i, ring) in rings.iter().enumerate() {
		if ring.len() < 3 {
			continue;
		}
		if i > 0 && !points.is_empty() {
			holes.push(points.len() as u32);
		}
		points.extend(ring.iter().copied());
	}
	if points.len() < 3 || points.iter().flatten().any(|c| !c.is_finite()) {
		return Vec::new();
	}
	let mut indices: Vec<u32> = Vec::new();
	earcut::Earcut::new().earcut(points.iter().copied(), &holes, &mut indices);
	indices
		.as_chunks::<3>()
		.0
		.iter()
		.map(|t| [points[t[0] as usize], points[t[1] as usize], points[t[2] as usize]])
		.collect()
}

/// A face on a surface not laid flat here: its loops on the plane that fits them best -- Newell's
/// normal -- triangulated there and drawn flat, the chord of whatever curve it has.
fn flat(rings: &[Vec<Vec3>], out: &mut Vec<[Vec3; 3]>) {
	let Some(outline) = rings.first() else { return };
	let mut normal = [0.0; 3];
	for (i, a) in outline.iter().enumerate() {
		let b = outline[(i + 1) % outline.len()];
		normal[0] += (a[1] - b[1]) * (a[2] + b[2]);
		normal[1] += (a[2] - b[2]) * (a[0] + b[0]);
		normal[2] += (a[0] - b[0]) * (a[1] + b[1]);
	}
	if dot(normal, normal) < 1e-18 {
		return;
	}
	let axis = unit(normal);
	let x = unit(if axis[0].abs() < 0.9 {
		cross(axis, [1.0, 0.0, 0.0])
	} else {
		cross(axis, [0.0, 1.0, 0.0])
	});
	let plane = Surface::Plane { origin: outline[0], x, y: cross(axis, x) };
	let mut flat: Vec<Vec<[f64; 2]>> =
		rings.iter().map(|r| r.iter().map(|p| plane.flatten(*p)).collect()).collect();
	// The flat triangles back in space by the rings' own points, not the plane's, so the patch keeps
	// the corners it has rather than being pressed onto one plane. Paired before the rings are
	// reordered, while each flat ring still stands beside the ring it came from.
	let lookup: Vec<([f64; 2], Vec3)> =
		flat.iter().flatten().copied().zip(rings.iter().flatten().copied()).collect();
	outline_first(&mut flat);
	for t in cut(&flat) {
		let back = |q: [f64; 2]| {
			lookup.iter().find(|(p, _)| *p == q).map_or_else(|| plane.place(q), |(_, s)| *s)
		};
		out.push(t.map(back));
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_square_with_a_square_hole_is_eight_triangles() {
		let outer = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
		let hole = vec![[1.0, 1.0], [1.0, 3.0], [3.0, 3.0], [3.0, 1.0]];
		assert_eq!(cut(&[outer, hole]).len(), 8);
	}

	#[test]
	fn a_band_between_two_circles_lies_on_its_cylinder() {
		let surface = Surface::Cone {
			origin: [0.0; 3],
			axis: [0.0, 0.0, 1.0],
			x: [1.0, 0.0, 0.0],
			y: [0.0, 1.0, 0.0],
			radius: 2.0,
			slope: 0.0,
		};
		let circle =
			|z: f64| (0..24).map(|i| surface.place([TAU * f64::from(i) / 24.0, z])).collect::<Vec<_>>();
		let mut out = Vec::new();
		face(&surface, &[circle(0.0), circle(3.0)], &mut out);
		assert!(!out.is_empty());
		for p in out.iter().flatten() {
			assert!(((p[0] * p[0] + p[1] * p[1]).sqrt() - 2.0).abs() < 1e-9, "on the cylinder: {p:?}");
		}
	}
}

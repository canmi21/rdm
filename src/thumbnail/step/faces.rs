//! A STEP part's faces as triangles. Each face is its surface bounded by loops of edges; the loops
//! are laid flat in the surface's own parameters -- a plane's two axes, a cylinder's or cone's angle
//! and height, a sphere's or torus's two angles, a B-spline patch's own two found by searching it
//! -- triangulated there with earcut, subdivided, and each corner put back on the surface, so a
//! cylinder is round and a fillet curves rather than a fan of chords crossing it. A surface of no
//! kind read here is laid flat on the plane that fits its loops best and drawn as that flat patch.
//! A face that cannot be laid flat is passed over; the card is a glance, not a measurement.

use super::{Model, Value, Vec3, add, cross, dot, scale, sub, unit};

use std::f64::consts::TAU;

/// How far a triangle's edge may stand off the surface, as a share of the whole part's size, before
/// it is split: a card is a few hundred pixels across, and the light is blended across a triangle
/// from the surface's own normals, so a coarse cut still reads smooth.
const SAG: f64 = 1.0 / 300.0;
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
	/// A B-spline patch, its own parameters found for each point by searching it.
	Patch(super::spline::Patch),
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
			"B_SPLINE_SURFACE_WITH_KNOTS" => Surface::Patch(super::spline::Patch::of(model, values)?),
			_ => return None,
		})
	}

	/// How far each parameter runs before it comes round to where it started: a turn for an angle,
	/// the domain for a closed patch, and nothing for a parameter that does not wrap.
	fn periods(&self) -> [Option<f64>; 2] {
		match self {
			Surface::Plane { .. } => [None, None],
			Surface::Cone { .. } => [Some(TAU), None],
			Surface::Sphere { .. } | Surface::Torus { .. } => [Some(TAU), Some(TAU)],
			Surface::Patch(patch) => {
				let period = |k: usize| patch.closed[k].then(|| patch.domain[k].1 - patch.domain[k].0);
				[period(0), period(1)]
			}
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
			Surface::Patch(patch) => patch.find(p, None),
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
			Surface::Patch(patch) => patch.at([u, v]),
		}
	}

	/// The surface's normal at a point of its parameters, either way out; the light is two-sided.
	fn normal(&self, [u, v]: [f64; 2]) -> Vec3 {
		let out = |x: Vec3, y: Vec3| add(scale(x, u.cos()), scale(y, u.sin()));
		match self {
			Surface::Plane { x, y, .. } => cross(*x, *y),
			Surface::Cone { axis, x, y, slope, .. } => sub(out(*x, *y), scale(*axis, *slope)),
			Surface::Sphere { origin, .. } => sub(self.place([u, v]), *origin),
			Surface::Torus { origin, x, y, major, .. } => {
				sub(self.place([u, v]), add(*origin, scale(out(*x, *y), *major)))
			}
			Surface::Patch(patch) => {
				let [(u0, u1), (v0, v1)] = patch.domain;
				let (hu, hv) = ((u1 - u0) * 1e-3, (v1 - v0) * 1e-3);
				let du = sub(patch.at([u + hu, v]), patch.at([u - hu, v]));
				let dv = sub(patch.at([u, v + hv]), patch.at([u, v - hv]));
				cross(du, dv)
			}
		}
	}
}

/// Triangles with the surface's normal at each corner, which the rasterizer lights smooth.
#[derive(Default)]
pub struct Shaded {
	pub triangles: Vec<[Vec3; 3]>,
	pub normals: Vec<[Vec3; 3]>,
}

impl Shaded {
	fn push(&mut self, surface: &Surface, t: [[f64; 2]; 3]) {
		self.triangles.push(t.map(|p| surface.place(p)));
		self.normals.push(t.map(|p| surface.normal(p)));
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

/// A wrapping parameter made continuous along a loop, so a loop around a cylinder runs a whole turn
/// rather than jumping back at the seam.
fn unwrap(points: &mut [[f64; 2]], k: usize, period: f64) {
	for i in 1..points.len() {
		let step = points[i][k] - points[i - 1][k];
		points[i][k] -= period * (step / period).round();
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
pub fn triangles(model: &Model) -> Shaded {
	let mut ids: Vec<u64> = model
		.entities
		.iter()
		.filter(|(_, (name, _))| name == "ADVANCED_FACE" || name == "FACE_SURFACE")
		.map(|(id, _)| *id)
		.collect();
	ids.sort_unstable();
	let step = ids.len().div_ceil(MOST_FACES).max(1);
	let mut out = Shaded::default();
	let tolerance = SAG * size(model);
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
			Some(surface) => face(&surface, &rings, tolerance, &mut out),
			None => flat(&rings, &mut out),
		}
	}
	out
}

/// A face on a surface this file can lay flat.
fn face(surface: &Surface, rings: &[Vec<Vec3>], tolerance: f64, out: &mut Shaded) {
	let periods = surface.periods();
	let mut flat: Vec<Vec<[f64; 2]>> = rings
		.iter()
		.map(|ring| match surface {
			// A patch is searched along the loop, each point from the last.
			Surface::Patch(patch) => {
				let mut last = None;
				ring
					.iter()
					.map(|p| {
						let at = patch.find(*p, last);
						last = Some(at);
						at
					})
					.collect()
			}
			_ => ring.iter().map(|p| surface.flatten(*p)).collect(),
		})
		.collect();
	for ring in &mut flat {
		for (k, period) in periods.iter().enumerate() {
			if let Some(period) = period {
				unwrap(ring, k, *period);
			}
		}
	}
	// A band round a cylinder, bounded by two circles and no seam: each circle lays flat as a line,
	// with nothing between them to triangulate, so the band is a strip from one to the next.
	if let Some(period) = periods[0]
		&& flat.len() >= 2
		&& flat.iter().all(|r| {
			let (low, high) = span(r, 0);
			high - low > 0.95 * period
		}) {
		let start = span(&flat[0], 0).0;
		let mut heights: Vec<f64> =
			flat.iter().map(|r| span(r, 1)).map(|(l, h)| (l + h) / 2.0).collect();
		heights.sort_by(f64::total_cmp);
		for pair in heights.windows(2) {
			let (v0, v1) = (pair[0], pair[1]);
			let steps = 48;
			for i in 0..steps {
				let u0 = start + period * i as f64 / steps as f64;
				let u1 = start + period * (i + 1) as f64 / steps as f64;
				out.push(surface, [[u0, v0], [u1, v0], [u1, v1]]);
				out.push(surface, [[u0, v0], [u1, v1], [u0, v1]]);
			}
		}
		return;
	}
	outline_first(&mut flat);
	// A hole that came out a period away from the outer loop is brought back beside it.
	if let Some(outer) = flat.first().cloned() {
		for ring in flat.iter_mut().skip(1) {
			for (k, period) in periods.iter().enumerate() {
				let Some(period) = period else { continue };
				let (low, high) = span(&outer, k);
				let (l, h) = span(ring, k);
				let shift = period * (((low + high) / 2.0 - (l + h) / 2.0) / period).round();
				for p in ring.iter_mut() {
					p[k] += shift;
				}
			}
		}
	}
	for triangle in cut(&flat) {
		split(surface, tolerance, triangle, 0, out);
	}
}

/// A triangle in parameters, split while the surface stands further than `tolerance` off the
/// middle of any of its edges, then put on the surface. A flat face is never split; a tight fillet
/// is split to `DEPTH`.
fn split(surface: &Surface, tolerance: f64, t: [[f64; 2]; 3], depth: usize, out: &mut Shaded) {
	let mid = |a: [f64; 2], b: [f64; 2]| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
	let placed = t.map(|p| surface.place(p));
	let sags = |i: usize, j: usize| {
		let chord = scale(add(placed[i], placed[j]), 0.5);
		super::dist(surface.place(mid(t[i], t[j])), chord) > tolerance
	};
	if depth >= DEPTH || !(sags(0, 1) || sags(1, 2) || sags(2, 0)) {
		out.push(surface, t);
		return;
	}
	let (ab, bc, ca) = (mid(t[0], t[1]), mid(t[1], t[2]), mid(t[2], t[0]));
	for piece in [[t[0], ab, ca], [ab, t[1], bc], [ca, bc, t[2]], [ab, bc, ca]] {
		split(surface, tolerance, piece, depth + 1, out);
	}
}

/// How big the part is: the diagonal of the box around its points.
fn size(model: &Model) -> f64 {
	let (mut low, mut high) = ([f64::MAX; 3], [f64::MIN; 3]);
	for (name, values) in model.entities.values() {
		if name != "CARTESIAN_POINT" {
			continue;
		}
		let Some(p) = values.get(1).and_then(super::triple) else { continue };
		for k in 0..3 {
			low[k] = low[k].min(p[k]);
			high[k] = high[k].max(p[k]);
		}
	}
	if low[0] > high[0] { 1.0 } else { super::dist(low, high).max(f64::EPSILON) }
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
fn flat(rings: &[Vec<Vec3>], out: &mut Shaded) {
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
		out.triangles.push(t.map(back));
		out.normals.push([axis; 3]);
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
		let mut out = Shaded::default();
		face(&surface, &[circle(0.0), circle(3.0)], 0.01, &mut out);
		assert!(!out.triangles.is_empty());
		for p in out.triangles.iter().flatten() {
			assert!(((p[0] * p[0] + p[1] * p[1]).sqrt() - 2.0).abs() < 1e-9, "on the cylinder: {p:?}");
		}
	}
}

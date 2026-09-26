//! B-splines, rational or not: a curve evaluated at a parameter, and a surface patch evaluated at two
//! and found the parameters of a point near it. The finding is a coarse grid for the nearest sample
//! and a few Newton steps from there, which is plenty for laying a face's loops flat on the patch.

use super::{Model, Value, Vec3, dist, sub};

/// A control point with its weight, the coordinates already multiplied by it.
type Weighted = [f64; 4];

/// The knot vector a list of multiplicities and a list of distinct knots spell out.
pub fn knots(multiplicities: &[Value], values: &[Value]) -> Option<Vec<f64>> {
	let mut out = Vec::new();
	for (m, k) in multiplicities.iter().zip(values) {
		for _ in 0..m.num()? as usize {
			out.push(k.num()?);
		}
	}
	Some(out)
}

pub fn weighted(point: Vec3, weight: f64) -> Weighted {
	[point[0] * weight, point[1] * weight, point[2] * weight, weight]
}

fn project(p: Weighted) -> Vec3 {
	if p[3].abs() < 1e-12 { [p[0], p[1], p[2]] } else { [p[0] / p[3], p[1] / p[3], p[2] / p[3]] }
}

/// de Boor's algorithm in homogeneous coordinates, `points.len() + degree + 1` knots.
pub fn de_boor(degree: usize, points: &[Weighted], knots: &[f64], t: f64) -> Weighted {
	let mut span = degree;
	while span + 1 < points.len() && knots[span + 1] <= t {
		span += 1;
	}
	let mut d: Vec<Weighted> = (0..=degree).map(|j| points[j + span - degree]).collect();
	for r in 1..=degree {
		for j in (r..=degree).rev() {
			let i = j + span - degree;
			let denominator = knots[i + degree + 1 - r] - knots[i];
			let alpha = if denominator.abs() < 1e-12 { 0.0 } else { (t - knots[i]) / denominator };
			let before = d[j - 1];
			for (c, was) in d[j].iter_mut().zip(before) {
				*c = was * (1.0 - alpha) + *c * alpha;
			}
		}
	}
	d[degree]
}

/// A curve's points between the ends of its domain.
pub fn curve(
	degree: usize,
	points: &[Weighted],
	knots: &[f64],
	samples: usize,
) -> Option<Vec<Vec3>> {
	if points.len() <= degree || knots.len() != points.len() + degree + 1 {
		return None;
	}
	let (low, high) = (knots[degree], knots[points.len()]);
	Some(
		(0..=samples)
			.map(|i| {
				project(de_boor(degree, points, knots, low + (high - low) * i as f64 / samples as f64))
			})
			.collect(),
	)
}

/// A B-spline surface: rows of weighted control points along u, each row along v.
pub struct Patch {
	degree: [usize; 2],
	grid: Vec<Vec<Weighted>>,
	knots: [Vec<f64>; 2],
	pub domain: [(f64, f64); 2],
	pub closed: [bool; 2],
}

/// How many samples a side the nearest point is first looked for among.
const SEEDS: usize = 14;
const NEWTON: usize = 8;

impl Patch {
	/// From a `B_SPLINE_SURFACE_WITH_KNOTS`'s parameters as the simple entity lays them out, with a
	/// rational one's weights after them.
	pub fn of(model: &Model, values: &[Value]) -> Option<Patch> {
		let degree = [values.get(1)?.num()? as usize, values.get(2)?.num()? as usize];
		let weights = values.get(13).map(Value::list);
		let grid: Vec<Vec<Weighted>> = values
			.get(3)?
			.list()
			.iter()
			.enumerate()
			.map(|(i, row)| {
				row
					.list()
					.iter()
					.enumerate()
					.map(|(j, v)| {
						let w = weights.and_then(|w| w.get(i)?.list().get(j)?.num()).unwrap_or(1.0);
						Some(weighted(model.point(v.id()?)?, w))
					})
					.collect::<Option<Vec<_>>>()
			})
			.collect::<Option<_>>()?;
		let flag = |at: usize| matches!(values.get(at), Some(Value::Other(f)) if f == ".T.");
		let knots_u = knots(values.get(8)?.list(), values.get(10)?.list())?;
		let knots_v = knots(values.get(9)?.list(), values.get(11)?.list())?;
		let (rows, columns) = (grid.len(), grid.first()?.len());
		if rows <= degree[0]
			|| columns <= degree[1]
			|| grid.iter().any(|r| r.len() != columns)
			|| knots_u.len() != rows + degree[0] + 1
			|| knots_v.len() != columns + degree[1] + 1
		{
			return None;
		}
		let domain = [(knots_u[degree[0]], knots_u[rows]), (knots_v[degree[1]], knots_v[columns])];
		Some(Patch { degree, grid, knots: [knots_u, knots_v], domain, closed: [flag(5), flag(6)] })
	}

	pub fn at(&self, [u, v]: [f64; 2]) -> Vec3 {
		let clamp = |t: f64, (low, high): (f64, f64)| t.clamp(low, high);
		let (u, v) = (clamp(u, self.domain[0]), clamp(v, self.domain[1]));
		let column: Vec<Weighted> =
			self.grid.iter().map(|row| de_boor(self.degree[1], row, &self.knots[1], v)).collect();
		project(de_boor(self.degree[0], &column, &self.knots[0], u))
	}

	/// The parameters of the point on the patch nearest `p`, searched for from `near` when a point
	/// close by was just found -- the next point along a loop -- and from a grid of samples when not.
	pub fn find(&self, p: Vec3, near: Option<[f64; 2]>) -> [f64; 2] {
		let [(u0, u1), (v0, v1)] = self.domain;
		let seeded = near.map(|at| self.settle(p, at)).filter(|at| {
			// A seed that settled far from the point was on the wrong side of something.
			dist(self.at(*at), p) < 1e-3 * (1.0 + dist(self.at([u0, v0]), self.at([u1, v1])))
		});
		if let Some(at) = seeded {
			return at;
		}
		let mut best = ([u0, v0], f64::MAX);
		for i in 0..=SEEDS {
			for j in 0..=SEEDS {
				let at =
					[u0 + (u1 - u0) * i as f64 / SEEDS as f64, v0 + (v1 - v0) * j as f64 / SEEDS as f64];
				let d = dist(self.at(at), p);
				if d < best.1 {
					best = (at, d);
				}
			}
		}
		self.settle(p, best.0)
	}

	/// Newton's steps from `at` toward the point on the patch nearest `p`.
	fn settle(&self, p: Vec3, mut at: [f64; 2]) -> [f64; 2] {
		let [(u0, u1), (v0, v1)] = self.domain;
		let (hu, hv) = ((u1 - u0) * 1e-4, (v1 - v0) * 1e-4);
		for _ in 0..NEWTON {
			let here = self.at(at);
			let du =
				sub(self.at([at[0] + hu, at[1]]), self.at([at[0] - hu, at[1]])).map(|c| c / (2.0 * hu));
			let dv =
				sub(self.at([at[0], at[1] + hv]), self.at([at[0], at[1] - hv])).map(|c| c / (2.0 * hv));
			let r = sub(p, here);
			let dot = |a: Vec3, b: Vec3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
			let (a, b, c) = (dot(du, du), dot(du, dv), dot(dv, dv));
			let (e, f) = (dot(du, r), dot(dv, r));
			let det = a * c - b * b;
			if det.abs() < 1e-18 {
				break;
			}
			let step = [(c * e - b * f) / det, (a * f - b * e) / det];
			at = [(at[0] + step[0]).clamp(u0, u1), (at[1] + step[1]).clamp(v0, v1)];
			if step[0].abs() < hu && step[1].abs() < hv {
				break;
			}
		}
		at
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_point_on_a_bilinear_patch_is_found_at_its_parameters() {
		let grid = vec![
			vec![weighted([0.0, 0.0, 0.0], 1.0), weighted([0.0, 1.0, 0.0], 1.0)],
			vec![weighted([2.0, 0.0, 1.0], 1.0), weighted([2.0, 1.0, 1.0], 1.0)],
		];
		let patch = Patch {
			degree: [1, 1],
			grid,
			knots: [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]],
			domain: [(0.0, 1.0), (0.0, 1.0)],
			closed: [false, false],
		};
		let p = patch.at([0.3, 0.8]);
		let found = patch.find(p, None);
		let again = patch.find(patch.at([0.35, 0.75]), Some(found));
		assert!((again[0] - 0.35).abs() < 1e-6 && (again[1] - 0.75).abs() < 1e-6, "{again:?}");
		assert!((found[0] - 0.3).abs() < 1e-6 && (found[1] - 0.8).abs() < 1e-6, "{found:?}");
	}
}

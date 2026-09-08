//! Property-based fuzz corpus for the CSG boolean pipeline (PR-07, and
//! the standing corpus for K-01's robustness work).
//!
//! Random primitive soups are booleaned under all three operations and
//! the outputs are checked against invariants that must hold for *any*
//! pair of closed operands:
//!
//! 1. **Structural sanity** — every index in range, every coordinate
//!    finite (no NaN/inf).
//! 2. **Volume monotonicity** (exact at the tessellated-polygon level):
//!    `vol(A ∪ B) ≥ max(vol A, vol B)`,
//!    `vol(A − B) ≤ vol A`,
//!    `vol(A ∩ B) ≤ min(vol A, vol B)`,
//!    and all results non-negative.
//! 3. **Outward orientation** — signed volume must be positive.
//! 4. **Error discipline** — the only permitted failure is an empty
//!    result (e.g. disjoint intersection, or B fully swallowing A);
//!    anything else (panic, other error kinds) is a bug.
//!
//! Deterministic by default: seeds are derived from a fixed base. Set
//! `FORGE_FUZZ_SEED` (hex or decimal) to explore a different stream;
//! failures print the seed + case number to reproduce.
//!
//! T-junction watertightness is deliberately *not* asserted yet: K-02
//! documents that booleans are volume-exact but topologically open; the
//! statistic is printed in verbose mode (`FUZZ_VERBOSE=1`) to track it.

use forge_core::{Point3, TessellationConfig, Vector3};
use forge_geometry::{boolean, primitives, CsgOp, TriMesh};

/// Coarse tessellation keeps the corpus fast while still exercising
/// curved-surface booleans.
const CFG: TessellationConfig = TessellationConfig {
    chord_tolerance_mm: 0.5,
    max_segment_angle_rad: 15.0_f64.to_radians(),
    max_segments_per_circle: 64,
    min_segments_per_circle: 16,
};

/// Deterministic xorshift64* — no external RNG dependency.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform `f64` in `[lo, hi]`.
    fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        let u = (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        lo + u * (hi - lo)
    }

    fn point(&mut self, spread: f64) -> Point3 {
        Point3::new(
            self.uniform(-spread, spread),
            self.uniform(-spread, spread),
            self.uniform(-spread, spread),
        )
    }
}

/// One random closed primitive near the origin.
fn random_primitive(rng: &mut Rng) -> TriMesh {
    match rng.next_u64() % 4 {
        0 => primitives::box_from_center_extents(
            rng.point(25.0),
            Vector3::new(
                rng.uniform(6.0, 30.0),
                rng.uniform(6.0, 30.0),
                rng.uniform(6.0, 30.0),
            ),
        ),
        1 => primitives::sphere(rng.point(25.0), rng.uniform(5.0, 18.0), &CFG),
        2 => primitives::cylinder(
            rng.point(25.0),
            rng.uniform(4.0, 12.0),
            rng.uniform(8.0, 30.0),
            &CFG,
        ),
        _ => primitives::cone(
            rng.point(25.0),
            rng.uniform(5.0, 15.0),
            rng.uniform(1.0, 10.0),
            rng.uniform(8.0, 28.0),
            &CFG,
        ),
    }
}

/// Structural sanity: valid indices, finite coordinates.
fn validate_structure(mesh: &TriMesh, ctx: &str) {
    let nv = mesh.positions.len();
    assert!(nv > 0, "{ctx}: no vertices");
    for (i, p) in mesh.positions.iter().enumerate() {
        assert!(
            p.x.is_finite() && p.y.is_finite() && p.z.is_finite(),
            "{ctx}: NaN/inf coordinate at vertex {i}: {p:?}"
        );
    }
    for (i, &idx) in mesh.indices.iter().enumerate() {
        assert!(
            (idx as usize) < nv,
            "{ctx}: index {idx} out of range ({} verts) at slot {i}",
            nv
        );
    }
}

#[test]
fn fuzz_csg_corpus() {
    let verbose = std::env::var("FUZZ_VERBOSE").is_ok();
    let base = std::env::var("FORGE_FUZZ_SEED")
        .ok()
        .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0xF0CA_D0CA_D00D_0007);
    const CASES: u64 = 48;

    let mut closed_count = 0usize;
    let mut ok_count = 0usize;
    let mut empty_count = 0usize;

    for case in 0..CASES {
        let seed = base.wrapping_add(case.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut rng = Rng(seed);
        let a = random_primitive(&mut rng);
        let b = random_primitive(&mut rng);
        let va = a.volume_signed();
        let vb = b.volume_signed();
        assert!(va > 0.0 && vb > 0.0, "operands must be positive solids");
        // Volume bounds are exact at the polygon level; only rounding is
        // tolerated.
        let tol = 1e-6 * (1.0 + va.abs() + vb.abs());

        for op in [CsgOp::Union, CsgOp::Difference, CsgOp::Intersection] {
            let ctx = format!("seed {seed:#x} case {case} op {op}");
            match boolean(&a, &b, op) {
                Ok(mesh) => {
                    ok_count += 1;
                    validate_structure(&mesh, &ctx);
                    let v = mesh.volume_signed();
                    if mesh.is_closed() {
                        closed_count += 1;
                    }
                    match op {
                        CsgOp::Union => assert!(
                            v >= va.max(vb) - tol,
                            "{ctx}: union volume {v} below operand max {}",
                            va.max(vb)
                        ),
                        CsgOp::Difference => assert!(
                            v <= va + tol,
                            "{ctx}: difference volume {v} above minuend {va}"
                        ),
                        CsgOp::Intersection => assert!(
                            v <= va.min(vb) + tol,
                            "{ctx}: intersection volume {v} above operand min {}",
                            va.min(vb)
                        ),
                    }
                    assert!(
                        v >= -tol,
                        "{ctx}: negative result volume {v} (inward orientation?)"
                    );
                }
                Err(e) => {
                    // Only a genuinely empty result may fail.
                    empty_count += 1;
                    let msg = format!("{e}");
                    assert!(
                        msg.contains("no output") || msg.contains("empty"),
                        "{ctx}: unexpected error: {e}"
                    );
                }
            }
        }
    }

    if verbose {
        println!(
            "fuzz_csg_corpus: {ok_count} ok, {empty_count} empty, \
             {closed_count}/{ok_count} strictly closed (T-junctions: {})",
            ok_count - closed_count
        );
    }
}

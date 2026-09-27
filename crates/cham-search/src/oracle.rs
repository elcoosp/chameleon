//! Independent validation oracles (SPECS/06 §5): a small enumerative Nash
//! solver for matrix games (support enumeration — exact for ≤ 5×5), used
//! to pin the FMBR/RNR machinery on single-round matrix-ized spots.
//!
//! The AGPL `postflop-solver` dev-time oracle (SPECS/06 §5.3) is NEVER
//! linked or shipped — it is an offline documentation procedure only
//! (cargo-deny tripwire in deny.toml; AGPL-3.0 is in the license deny list).
//!
//! C-5 fix (2026-09-27): the previous version was not an independent
//! validator. Three problems, all fixed here:
//!
//! 1. `solve_matrix` kept the support pair with the MAX `v` (line 23:
//!    `v > *bv`). Single-cell supports pass the row-only check trivially
//!    and win. For the matching-pennies matrix [[1,-1],[-1,1]] this
//!    returned `(v=1.0, p=[1,0], q=[1,0])` — the true Nash value is 0.0.
//!    Now: `solve_support` only returns VERIFIED equilibria, and
//!    `solve_matrix` returns the FIRST one (any verified equilibrium is a
//!    valid answer; maximizing is wrong).
//!
//! 2. `solve_support` verified only the row side ("no row beats v against
//!    q"). The column side — "no column best-responds below v against p"
//!    — was never checked. Now both are checked.
//!
//! 3. The "normalization row" was `m[n_eq][c-1] = 1.0`, which encodes
//!    "q_last = 1", not "Σ q = 1". That is why full-support pairs were
//!    rejected. Now both p and q are solved by a proper linear system:
//!      - q: equalize A[i]·q over i ∈ rs, + Σ q = 1
//!      - p: equalize A^T[j]·p over j ∈ cs, + Σ p = 1
//!    Both are verified against the FULL payoff matrix afterward.

/// Solve a zero-sum matrix game (row = maximizer) exactly by support
/// enumeration. Returns `(row_value, row_strategy, col_strategy)` — any
/// verified Nash equilibrium. Returns `None` if the matrix is malformed
/// or if no verified equilibrium is found (which, for a finite game,
/// should only happen on numerically degenerate inputs).
pub fn solve_matrix(a: &[Vec<f64>]) -> Option<(f64, Vec<f64>, Vec<f64>)> {
    let rows = a.len();
    let cols = a.first()?.len();
    if rows > 5 || cols > 5 || rows == 0 || cols == 0 {
        return None;
    }
    // rectangularity sanity
    if a.iter().any(|r| r.len() != cols) {
        return None;
    }
    let row_supports = support_sets(rows);
    let col_supports = support_sets(cols);
    for rs in &row_supports {
        for cs in &col_supports {
            if let Some(eq) = solve_support(a, rs, cs) {
                // First verified pair wins (any verified equilibrium is fine).
                return Some(eq);
            }
        }
    }
    None
}

fn support_sets(n: usize) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    for mask in 1u32..(1u32 << n) {
        let s: Vec<usize> = (0..n).filter(|i| mask & (1 << i) != 0).collect();
        out.push(s);
    }
    out
}

/// Solve the equalizer system on a support pair and VERIFY that (p, q, v)
/// is a Nash equilibrium of the full matrix.
fn solve_support(
    a: &[Vec<f64>],
    rs: &[usize],
    cs: &[usize],
) -> Option<(f64, Vec<f64>, Vec<f64>)> {
    let rows = a.len();
    let cols = a.first()?.len();
    let r = rs.len();
    let c = cs.len();

    // ----- column strategy q over cs, zero outside -----
    // Equalize row payoffs: for i1, i2 ∈ rs:  Σ_j (a[i1][j]-a[i2][j]) q_j = 0
    // plus Σ q = 1.
    let q_support = solve_equalizer(
        r,
        c,
        |e, j| {
            let i1 = rs[e];
            let i2 = rs[e + 1];
            a[i1][cs[j]] - a[i2][cs[j]]
        },
    )?;
    let mut q = vec![0.0; cols];
    for (j, &cj) in cs.iter().enumerate() {
        q[cj] = q_support[j];
    }
    // sanity: non-negative, sums to 1
    if q.iter().any(|&x| x < -1e-9) {
        return None;
    }
    let qs: f64 = q.iter().sum();
    if (qs - 1.0).abs() > 1e-6 {
        return None;
    }

    // ----- row strategy p over rs, zero outside -----
    // Equalize column payoffs: for j1, j2 ∈ cs:  Σ_i (a[i][j1]-a[i][j2]) p_i = 0
    // plus Σ p = 1.
    let p_support = solve_equalizer(
        c,
        r,
        |e, i| {
            let j1 = cs[e];
            let j2 = cs[e + 1];
            a[rs[i]][j1] - a[rs[i]][j2]
        },
    )?;
    let mut p = vec![0.0; rows];
    for (i, &ri) in rs.iter().enumerate() {
        p[ri] = p_support[i];
    }
    if p.iter().any(|&x| x < -1e-9) {
        return None;
    }
    let ps: f64 = p.iter().sum();
    if (ps - 1.0).abs() > 1e-6 {
        return None;
    }

    // ----- value: row's equalized payoff vs q -----
    let value_at_row = |i: usize| -> f64 { (0..cols).map(|j| a[i][j] * q[j]).sum() };
    let v = value_at_row(rs[0]);

    // ----- VERIFY row side: no row beats v against q -----
    for i in 0..rows {
        if value_at_row(i) > v + 1e-7 {
            return None;
        }
    }

    // ----- VERIFY column side: no column pays below v against p -----
    let value_at_col = |j: usize| -> f64 { (0..rows).map(|i| p[i] * a[i][j]).sum() };
    for j in 0..cols {
        if value_at_col(j) < v - 1e-7 {
            return None;
        }
    }

    Some((v, p, q))
}

/// Solve the (k-1)-indifference + normalization system for a mixture of
/// length `k` living on a support of size `k` (indexed 0..k). The
/// `coef(e, j)` closure returns the coefficient of mixture-component j in
/// indifference equation e (e ∈ 0..k-1). The final row is a full row of
/// ones (Σ x = 1) — the correct normalization.
fn solve_equalizer<F>(k: usize, _dim: usize, coef: F) -> Option<Vec<f64>>
where
    F: Fn(usize, usize) -> f64,
{
    if k == 0 {
        return None;
    }
    if k == 1 {
        return Some(vec![1.0]);
    }
    // Build the k×k system: rows 0..k-1 are indifference equations, last
    // row is [1,1,...,1 | 1].
    let mut m = vec![vec![0.0; k + 1]; k];
    for e in 0..(k - 1) {
        for j in 0..k {
            m[e][j] = coef(e, j);
        }
        m[e][k] = 0.0;
    }
    for j in 0..k {
        m[k - 1][j] = 1.0;
    }
    m[k - 1][k] = 1.0;
    gaussian_solve(&mut m, k)?;
    let mut out = vec![0.0; k];
    for j in 0..k {
        out[j] = m[j][k];
    }
    Some(out)
}

/// Gauss–Jordan solve of a k×(k+1) augmented system. Returns the solution
/// in column k of each row's pivot (row-reduced echelon form), or `None`
/// if singular.
fn gaussian_solve(m: &mut [Vec<f64>], k: usize) -> Option<()> {
    for col in 0..k {
        // pivot
        let mut piv = None;
        for r in col..k {
            if m[r][col].abs() > 1e-9 {
                piv = Some(r);
                break;
            }
        }
        let piv = piv?;
        m.swap(col, piv);
        let pivot = m[col][col];
        for c in col..=k {
            m[col][c] /= pivot;
        }
        for r in 0..k {
            if r != col && m[r][col].abs() > 1e-12 {
                let f = m[r][col];
                for c in col..=k {
                    m[r][c] -= f * m[col][c];
                }
            }
        }
    }
    Some(())
}

/// Reference spots (committed).
///
/// `reference_matrix_2x2` is **matching pennies**: [[1,-1],[-1,1]]. The
/// unique Nash equilibrium is p = q = [0.5, 0.5] with value **0.0**. This
/// is the test that would have caught C-5 — the previous `solve_matrix`
/// returned `(1.0, [1,0], [1,0])` for it.
pub fn reference_matrix_2x2() -> Vec<Vec<f64>> {
    vec![vec![1.0, -1.0], vec![-1.0, 1.0]]
}

/// A 2×2 matrix with a pure-strategy equilibrium at (row 0, col 0),
/// value 1.0. Used to exercise the pure-support path.
pub fn reference_matrix_2x2_pure() -> Vec<Vec<f64>> {
    vec![vec![1.0, 1.0], vec![1.0, -1.0]]
}

/// A 3×3 rock-paper-scissors-like cyclic game; value 0.0, uniform mix.
pub fn reference_matrix_3x3_rps() -> Vec<Vec<f64>> {
    vec![
        vec![0.0, -1.0, 1.0],
        vec![1.0, 0.0, -1.0],
        vec![-1.0, 1.0, 0.0],
    ]
}

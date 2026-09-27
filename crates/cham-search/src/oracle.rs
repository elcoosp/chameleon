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
//! 1. `solve_matrix` kept the support pair with the MAX `v`. Single-cell
//!    supports pass the row-only check trivially and win. For the
//!    matching-pennies matrix [[1,-1],[-1,1]] this returned
//!    `(v=1.0, p=[1,0], q=[1,0])` — the true Nash value is 0.0. Now:
//!    `solve_support` only returns VERIFIED equilibria, and `solve_matrix`
//!    returns the FIRST one (any verified equilibrium is a valid answer).
//!
//! 2. `solve_support` verified only the row side. The column side — "no
//!    column best-responds below v against p" — was never checked. Now
//!    both are checked against the full payoff matrix.
//!
//! 3. The "normalization row" was `m[n_eq][c-1] = 1.0`, which encoded
//!    "q_last = 1", not "Σ q = 1". Now the last row is a full row of ones
//!    (Σ x = 1), the correct normalization.
//!
//! Additionally fixed (2026-09-27, second pass): a support-pair with
//! |rs| ≠ |cs| used to panic with an out-of-bounds index. Standard
//! support enumeration requires |rs| = |cs| — we now skip unequal pairs,
//! which is sufficient to find at least one equilibrium of any finite
//! game.

/// Solve a zero-sum matrix game (row = maximizer) exactly by support
/// enumeration. Returns `(row_value, row_strategy, col_strategy)` — any
/// verified Nash equilibrium. Returns `None` if the matrix is malformed
/// or if no verified equilibrium is found.
pub fn solve_matrix(a: &[Vec<f64>]) -> Option<(f64, Vec<f64>, Vec<f64>)> {
    let rows = a.len();
    let cols = a.first()?.len();
    if rows > 5 || cols > 5 || rows == 0 || cols == 0 {
        return None;
    }
    // Rectangularity sanity: every row must have exactly `cols` entries.
    if a.iter().any(|r| r.len() != cols) {
        return None;
    }
    let row_supports = support_sets(rows);
    let col_supports = support_sets(cols);
    for rs in &row_supports {
        for cs in &col_supports {
            if let Some(eq) = solve_support(a, rs, cs) {
                // First verified pair wins (any verified equilibrium is
                // a valid answer — do NOT maximize over the pairs).
                return Some(eq);
            }
        }
    }
    None
}

/// Non-empty subsets of 0..n, in increasing-size order via the standard
/// bit-mask iteration. `support_sets(0)` returns an empty vector.
fn support_sets(n: usize) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    if n == 0 {
        return out;
    }
    for mask in 1u32..(1u32 << n) {
        let s: Vec<usize> = (0..n).filter(|i| mask & (1 << i) != 0).collect();
        out.push(s);
    }
    out
}

/// Solve the equalizer system on a support pair and VERIFY that (p, q, v)
/// is a Nash equilibrium of the full matrix.
///
/// Standard support enumeration restricts to |rs| == |cs|; unequal sizes
/// are skipped (the enumeration still finds at least one equilibrium of
/// any finite game within the equal-size support pairs).
fn solve_support(
    a: &[Vec<f64>],
    rs: &[usize],
    cs: &[usize],
) -> Option<(f64, Vec<f64>, Vec<f64>)> {
    let rows = a.len();
    let cols = a.first()?.len();
    let r = rs.len();
    let c = cs.len();
    if r != c {
        return None;
    }
    let k = r;

    // ---- column strategy q over cs, zero outside ----
    // System: (k-1) indifference equations  A[rs[e]]·q = A[rs[e+1]]·q,
    //         plus a full row of ones for Σ q = 1.
    let q_support = {
        let mut m = vec![vec![0.0; k + 1]; k];
        for e in 0..k.saturating_sub(1) {
            let i1 = rs[e];
            let i2 = rs[e + 1];
            for (j, &cj) in cs.iter().enumerate() {
                m[e][j] = a[i1][cj] - a[i2][cj];
            }
            m[e][k] = 0.0;
        }
        // Normalization row: sum of q components = 1 (full row of ones).
        for j in 0..k {
            m[k - 1][j] = 1.0;
        }
        m[k - 1][k] = 1.0;
        gaussian_solve(&mut m, k)?;
        let mut out = vec![0.0; k];
        for j in 0..k {
            out[j] = m[j][k];
        }
        out
    };

    // ---- row strategy p over rs, zero outside ----
    // System: (k-1) indifference equations  A[:,cs[e]]·p = A[:,cs[e+1]]·p,
    //         plus a full row of ones for Σ p = 1.
    let p_support = {
        let mut m = vec![vec![0.0; k + 1]; k];
        for e in 0..k.saturating_sub(1) {
            let j1 = cs[e];
            let j2 = cs[e + 1];
            for (i, &ri) in rs.iter().enumerate() {
                m[e][i] = a[ri][j1] - a[ri][j2];
            }
            m[e][k] = 0.0;
        }
        for i in 0..k {
            m[k - 1][i] = 1.0;
        }
        m[k - 1][k] = 1.0;
        gaussian_solve(&mut m, k)?;
        let mut out = vec![0.0; k];
        for i in 0..k {
            out[i] = m[i][k];
        }
        out
    };

    // ---- expand to full strategies (zero outside support) ----
    let mut p = vec![0.0; rows];
    for (i, &ri) in rs.iter().enumerate() {
        p[ri] = p_support[i];
    }
    let mut q = vec![0.0; cols];
    for (j, &cj) in cs.iter().enumerate() {
        q[cj] = q_support[j];
    }

    // ---- validity: non-negative, sums to 1 ----
    if p.iter().any(|&x| x < -1e-9) || q.iter().any(|&x| x < -1e-9) {
        return None;
    }
    let ps: f64 = p.iter().sum();
    let qs: f64 = q.iter().sum();
    if (ps - 1.0).abs() > 1e-6 || (qs - 1.0).abs() > 1e-6 {
        return None;
    }

    // ---- value: row's equalized payoff vs q ----
    let value_at_row = |i: usize| -> f64 { (0..cols).map(|j| a[i][j] * q[j]).sum() };
    let v = value_at_row(rs[0]);

    // ---- VERIFY row side: no row beats v against q ----
    for i in 0..rows {
        if value_at_row(i) > v + 1e-7 {
            return None;
        }
    }

    // ---- VERIFY column side: no column pays below v against p ----
    let value_at_col = |j: usize| -> f64 { (0..rows).map(|i| p[i] * a[i][j]).sum() };
    for j in 0..cols {
        if value_at_col(j) < v - 1e-7 {
            return None;
        }
    }

    Some((v, p, q))
}

/// Gauss–Jordan solve of a k×(k+1) augmented system. On success, m is in
/// reduced row-echelon form and the solution vector is m[·][k]. Returns
/// `None` if the system is singular (no unique solution).
fn gaussian_solve(m: &mut [Vec<f64>], k: usize) -> Option<()> {
    if k == 0 {
        return Some(());
    }
    for col in 0..k {
        // Find pivot in column `col` at row >= col.
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

/// Reference spot #1: **matching pennies** `[[1,-1],[-1,1]]`.
///
/// The unique Nash equilibrium is `p = q = [0.5, 0.5]` with value `0.0`.
/// This is the test that would have caught C-5 — the pre-fix `solve_matrix`
/// returned `(1.0, [1,0], [1,0])` for it (the matrix maximum, not the game
/// value).
pub fn reference_matrix_2x2() -> Vec<Vec<f64>> {
    vec![vec![1.0, -1.0], vec![-1.0, 1.0]]
}

/// Reference spot #2: `[[1,1],[1,-1]]`. Row 0 weakly dominates; the value
/// is 1.0 and a pure equilibrium is `p = q = [1,0]`.
pub fn reference_matrix_2x2_pure() -> Vec<Vec<f64>> {
    vec![vec![1.0, 1.0], vec![1.0, -1.0]]
}

/// Reference spot #3: rock-paper-scissors (cyclic, antisymmetric). The
/// unique equilibrium is uniform over all three pure strategies on both
/// sides; the value is 0.0.
pub fn reference_matrix_3x3_rps() -> Vec<Vec<f64>> {
    vec![
        vec![0.0, -1.0, 1.0],
        vec![1.0, 0.0, -1.0],
        vec![-1.0, 1.0, 0.0],
    ]
}

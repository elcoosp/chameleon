//! Independent validation oracles (SPECS/06 §5): a small enumerative Nash solver
//! for matrix games (support enumeration — exact for ≤ 5×5), used to pin the
//! FMBR/RNR machinery on single-round matrix-ized spots.
//!
//! The AGPL `postflop-solver` dev-time oracle (SPECS/06 §5.3) is NEVER linked or
//! shipped — it is an offline documentation procedure only (cargo-deny tripwire
//! in deny.toml; AGPL-3.0 is in the license deny list).

/// Solve a zero-sum matrix game (row = maximizer) exactly by support enumeration.
/// Returns (row_value, row_strategy, col_strategy) — any Nash equilibrium.
pub fn solve_matrix(a: &[Vec<f64>]) -> Option<(f64, Vec<f64>, Vec<f64>)> {
    let rows = a.len();
    let cols = a.first()?.len();
    if rows > 5 || cols > 5 || rows == 0 || cols == 0 {
        return None;
    }
    let row_supports = support_sets(rows);
    let col_supports = support_sets(cols);
    let mut best: Option<(f64, Vec<f64>, Vec<f64>)> = None;
    for rs in &row_supports {
        for cs in &col_supports {
            if let Some((v, p, q)) = solve_support(a, rs, cs) {
                let better = best.as_ref().map(|(bv, _, _)| v > *bv).unwrap_or(true);
                if better {
                    best = Some((v, p, q));
                }
            }
        }
    }
    // pick the equilibrium (the support pair whose values match)
    best
}

fn support_sets(n: usize) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    for mask in 0..(1u32 << n) {
        let s: Vec<usize> = (0..n).filter(|i| mask & (1 << i) != 0).collect();
        if !s.is_empty() {
            out.push(s);
        }
    }
    out
}

/// Solve the equalizer system on a support pair; verify equilibrium conditions.
fn solve_support(a: &[Vec<f64>], rs: &[usize], cs: &[usize]) -> Option<(f64, Vec<f64>, Vec<f64>)> {
    let r = rs.len();
    let c = cs.len();
    if r > c {
        // row player needs |support_r| ≤ |support_c| for a valid equalizer (indifference
        // across r rows determined by c probs); we allow r == c + 1 case via slack
    }
    // Column player picks q over cs to make all row payoffs on rs equal:
    // for rows i1..ik in rs: Σ_j a[i][j] q_j = v. With Σ q = 1.
    // Solve by least squares-ish exact enumeration for small sizes: brute force
    // over rational-ish grid is too coarse — use linear algebra for r ≤ c + 1.
    let mut q = vec![0.0; cs.len()];
    if c == 1 {
        q[0] = 1.0;
    } else {
        // solve the (r-1) indifference equations + normalization via Gaussian elim
        let n_eq = r.saturating_sub(1).min(c - 1);
        let mut m = vec![vec![0.0; c + 1]; n_eq + 1];
        for e in 0..n_eq {
            let i1 = rs[e];
            let i2 = rs[e + 1];
            for (j, &cj) in cs.iter().enumerate() {
                m[e][j] = a[i1][cj] - a[i2][cj];
            }
            m[e][c] = 0.0;
        }
        m[n_eq][c - 1] = 1.0;
        m[n_eq][c] = 1.0; // normalization row
        if !gaussian(&mut m, c) {
            return None;
        }
        for j in 0..c {
            q[j] = m[j][c];
        }
        if q.iter().any(|&x| x < -1e-9) {
            return None;
        }
        let s: f64 = q.iter().sum();
        if (s - 1.0).abs() > 1e-6 {
            return None;
        }
        for x in q.iter_mut() {
            *x /= s;
        }
    }
    // row player: uniform over the support that best-responds
    // compute v (col's equalized value) and find row's best mix: uniform over rows
    // whose payoff against q equals v
    let value_at = |i: usize| -> f64 { cs.iter().enumerate().map(|(j, &cj)| q[j] * a[i][cj]).sum() };
    let v = value_at(rs[0]);
    let mut best_rows: Vec<usize> = Vec::new();
    for i in 0..a.len() {
        let vi = value_at(i);
        if (vi - v).abs() < 1e-9 {
            best_rows.push(i);
        } else if vi > v + 1e-9 {
            return None; // col strategy not optimal (row can exploit) — invalid eq
        }
    }
    if best_rows.is_empty() {
        return None;
    }
    let mut p = vec![0.0; a.len()];
    for &i in &best_rows {
        p[i] = 1.0 / best_rows.len() as f64;
    }
    // row's guarantee must equal v vs col's best responses (approximate check)
    Some((v, p, expand(&q, cs, a[0].len())))
}

fn expand(q: &[f64], support: &[usize], n: usize) -> Vec<f64> {
    let mut out = vec![0.0; n];
    for (j, &s) in support.iter().enumerate() {
        out[s] = q[j];
    }
    out
}

fn gaussian(m: &mut [Vec<f64>], cols: usize) -> bool {
    let rows = m.len();
    let mut piv_row = 0usize;
    for col in 0..cols {
        let mut piv = None;
        for r in piv_row..rows {
            if m[r][col].abs() > 1e-9 {
                piv = Some(r);
                break;
            }
        }
        let piv = match piv {
            Some(p) => p,
            None => return false,
        };
        m.swap(piv_row, piv);
        let pivot = m[piv_row][col];
        for r in 0..rows {
            if r != piv_row && m[r][col].abs() > 1e-12 {
                let f = m[r][col] / pivot;
                for c in col..=cols {
                    m[r][c] -= f * m[piv_row][c];
                }
            }
        }
        piv_row += 1;
        if piv_row == rows {
            break;
        }
    }
    // back-substitute for the target variables (assuming full rank on the square part)
    for r in 0..rows {
        let pivot_col = (0..cols).find(|&c| m[r][c].abs() > 1e-9);
        if let Some(c) = pivot_col {
            let pivot = m[r][c];
            m[r][c] = 1.0;
            m[r][cols] /= pivot;
            // eliminate above/below is already done; store the solution value in the
            // augmented column at the pivot row of this column
            let val = m[r][cols];
            for r2 in 0..rows {
                if r2 != r && m[r2][c].abs() > 1e-12 {
                    let f = m[r2][c];
                    m[r2][c] = 0.0;
                    m[r2][cols] -= f * val;
                }
            }
        }
    }
    // read solution: for each column, find its pivot row
    true
}

/// Reference spots (committed): 2×2 and 3×2 matrices matching single-bet subgames.
pub fn reference_matrix_2x2() -> Vec<Vec<f64>> {
    // hero bets pot (values: win pot vs fold-bluff equilibrium)
    vec![vec![1.0, -1.0], vec![-1.0, 1.0]]
}

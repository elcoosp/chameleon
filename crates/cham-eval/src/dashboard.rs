//! Dashboard (SPECS/08 §8): trimmed 4-section static HTML + inline SVG frontier.
//! (1) headline card + verdicts (2) per-opponent winrate table (3) frontier chart
//! (4) ledger table. Router/search health moved to `chameleon trace`.

use crate::ingest::IngestSummary;

/// Render the dashboard HTML from a summary + frontier points + ledger rows.
pub fn render(
    summary: &IngestSummary,
    frontier: &[(f64, f64)],
    ledger_rows: &[(String, String, f64)],
) -> String {
    let mut html = String::with_capacity(8192);
    html.push_str(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>CHAMELEON dashboard</title>",
    );
    html.push_str("<style>body{font-family:monospace;margin:2em}table{border-collapse:collapse}td,th{border:1px solid #999;padding:4px 8px}</style>");
    html.push_str("</head><body>");
    // 1. headline card
    html.push_str("<h1>CHAMELEON — dashboard</h1>");
    html.push_str(&format!(
        "<p><b>baseline:</b> {} opponents, {} seatings ingested</p>",
        summary.by_label.len(),
        summary.by_label.values().map(|(_, _, s)| s).sum::<u64>()
    ));
    // 2. per-opponent winrate table
    html.push_str("<h2>2. per-opponent winrates (mb/seating, session-clustered CI inputs)</h2><table><tr><th>opponent</th><th>mb</th><th>se</th><th>seatings</th></tr>");
    for (label, (mb, se, seatings)) in &summary.by_label {
        html.push_str(&format!(
            "<tr><td>{label}</td><td>{mb:.1}</td><td>{se:.1}</td><td>{seatings}</td></tr>"
        ));
    }
    html.push_str("</table>");
    // 3. frontier chart (inline SVG): x = exploitability (LBR), y = winrate
    html.push_str("<h2>3. exploitation–exploitability frontier</h2>");
    html.push_str("<svg width=\"480\" height=\"320\" xmlns=\"http://www.w3.org/2000/svg\">");
    html.push_str("<rect width=\"480\" height=\"320\" fill=\"white\" stroke=\"black\"/>");
    let w = 400.0f64;
    let h = 260.0f64;
    let x_max = frontier.iter().map(|(x, _)| *x).fold(1.0f64, f64::max);
    let y_min = frontier.iter().map(|(_, y)| *y).fold(0.0f64, f64::min);
    let y_max = frontier.iter().map(|(_, y)| *y).fold(1.0f64, f64::max);
    for (x, y) in frontier {
        let px = 40.0 + (x / x_max) * w;
        let py = 300.0 - ((y - y_min) / (y_max - y_min + 1e-9)) * h;
        html.push_str(&format!(
            "<circle cx=\"{px:.1}\" cy=\"{py:.1}\" r=\"4\" fill=\"#0a7\"/>"
        ));
    }
    html.push_str("<text x=\"40\" y=\"316\" font-size=\"10\">exploitability (LBR bb) →</text>");
    html.push_str("<text x=\"4\" y=\"30\" font-size=\"10\">winrate ↑</text>");
    html.push_str("</svg>");
    // 4. ledger table
    html.push_str("<h2>4. ledger</h2><table><tr><th>run</th><th>type</th><th>delta mb</th></tr>");
    for (run, kind, delta) in ledger_rows {
        html.push_str(&format!(
            "<tr><td>{run}</td><td>{kind}</td><td>{delta:.1}</td></tr>"
        ));
    }
    html.push_str("</table></body></html>");
    html
}

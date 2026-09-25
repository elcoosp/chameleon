#!/usr/bin/env python3
"""Emit a criterion-result markdown table.

Reads `target/criterion/<bench>/{new,ci,base}/estimates.json` and prints a
table comparing each bench's current `new` mean against the `ci` baseline
(falling back to criterion's own `base`). Designed to be redirected into
$GITHUB_STEP_SUMMARY or piped to a file for local review.
"""
import json
import os
import glob
import platform
import sys

root = "target/criterion"

def read_mean(path):
    try:
        with open(path) as f:
            return json.load(f)["mean"]["point_estimate"]
    except Exception:
        return None

def human(seconds):
    if seconds >= 1e9:  return seconds / 1e9, "s"
    if seconds >= 1e6:  return seconds / 1e6, "ms"
    if seconds >= 1e3:  return seconds / 1e3, "µs"
    return seconds, "ns"

rows = []
for est in glob.glob(os.path.join(root, "**", "new", "estimates.json"), recursive=True):
    bench_dir = os.path.dirname(os.path.dirname(est))
    name = os.path.relpath(bench_dir, root)
    # keep only top-level benches (no nested dirs)
    if os.sep in name:
        continue
    mean = read_mean(est)
    if mean is None or mean <= 0:
        continue
    change = None
    for base_name in ("ci", "base"):
        base_est = os.path.join(bench_dir, base_name, "estimates.json")
        if os.path.exists(base_est):
            b = read_mean(base_est)
            if b and b > 0:
                change = (mean - b) / b * 100.0
                break
    v, u = human(mean)
    rows.append((name, v, u, change))

rows.sort()

out = sys.stdout
out.write("## Criterion bench summary\n\n")
if not rows:
    out.write("_(no `target/criterion/*/new/estimates.json` found — run `cargo bench` first)_\n")
else:
    out.write("| bench | mean | Δ vs baseline |\n")
    out.write("|---|---:|---:|\n")
    for name, v, u, c in rows:
        delta = f"{c:+.2f}%" if c is not None else "—"
        out.write(f"| `{name}` | {v:.3f} {u} | {delta} |\n")
    out.write("\n")
    out.write(f"_host: {platform.machine()} / {platform.system()}_\n")

# cham-rec

The flight recorder (SPECS/12): a leaf crate owning ALL structured JSONL event
writing. Run dir `artifacts/runs/<run_id>/events.jsonl`; envelope
`{"ts","run","kind","seq","data"}`; flush + fsync every 1000 records; append-
only — a corrupted tail stops the run, never truncates.

`schema.rs` holds the record-kind registry (single source of truth);
`validate.rs` is the offline validator wired into `chameleon verify`.

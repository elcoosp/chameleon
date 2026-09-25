# cham-cli (bin: `chameleon`)

Orchestration only (SPECS/09) — no business logic: no CFR, tracker, or stats
code lives here. The spec's §2 command list, wired to the crates:

verify · train-buckets · train-bp · train-router · collect · probe · ladder ·
ab · slumbot · play · trace · dashboard

Exit codes: 0 green, 1 failure, 2 budget refusal. Artifact-consuming commands
print their blake3 hashes. The justfile invokes everything through
`cargo run -q -p cham-cli --` (no bare binary calls).

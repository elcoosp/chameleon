CHAM := "cargo run -q -p cham-cli --"

test:
    cargo nextest run --workspace

bench:
    cargo bench --workspace

verify:
    just test && just bench && {{ CHAM }} verify --perf --count-infosets --proofs

fast:
    just verify && {{ CHAM }} probe && {{ CHAM }} ladder --fast && {{ CHAM }} dashboard

nightly:
    {{ CHAM }} ladder --full && {{ CHAM }} slumbot --seatings 20000 && {{ CHAM }} dashboard

mutants:
    cargo mutants -p cham-core -p cham-blueprint --in-diff-against HEAD

deny:
    cargo deny check

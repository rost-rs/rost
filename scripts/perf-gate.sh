#!/bin/sh
# Fails when the working tree executes more than THRESHOLD percent more instructions than
# BASE on any workload of benches/gate.rs.
#
#   scripts/perf-gate.sh [BASE] [THRESHOLD]      (defaults: origin/main, 3)
#
# Instruction counts come from valgrind (cachegrind): they do not depend on the machine,
# its load or the binary layout, so a reported change is a change in the code and the
# gate has no noise to mistake for a regression. Wall-clock time (`cargo bench --bench
# gate`) varies by 10-20% between identical builds on a laptop and is not used here.
# Each workload is measured as (setup + workload) - setup, with the same gate.rs on both
# builds.
set -eu
command -v valgrind >/dev/null || { echo "perf gate: valgrind is required" >&2; exit 2; }
base=${1:-origin/main}
threshold=${2:-3}
root=$(git rev-parse --show-toplevel)
work=$(mktemp -d)
trap 'git -C "$root" worktree remove --force "$work/base" >/dev/null 2>&1 || true; rm -rf "$work"' EXIT

git -C "$root" worktree add --detach "$work/base" "$base" >/dev/null
cp "$root/benches/gate.rs" "$work/base/benches/gate.rs"
if ! grep -q 'name = "gate"' "$work/base/Cargo.toml"; then
    printf '\n[[bench]]\nname = "gate"\nharness = false\nrequired-features = ["serde"]\n' >>"$work/base/Cargo.toml"
fi

# Asks cargo for the exact executable: a glob over target/ can pick up a stale build.
build() {
    (cd "$1" && CARGO_TARGET_DIR="$1/target" cargo bench --features serde --bench gate --no-run \
        --message-format=json 2>/dev/null) |
        sed -n 's/.*"executable":"\([^"]*\/gate-[^"]*\)".*/\1/p' | tail -n 1
}
base_bin=$(build "$work/base")
head_bin=$(build "$root")
[ -n "$base_bin" ] && [ -n "$head_bin" ] || { echo "perf gate: build failed" >&2; exit 2; }

# Instructions executed by `BIN run NAME`, from cachegrind's summary line.
instructions() {
    valgrind --tool=cachegrind --cache-sim=no --cachegrind-out-file=/dev/null "$1" run "$2" 2>&1 |
        sed -n 's/.*I *refs: *\([0-9,]*\).*/\1/p' | tr -d ','
}

for side in base head; do
    eval "bin=\$${side}_bin"
    setup=$(instructions "$bin" none)
    for name in $("$bin" list); do
        total=$(instructions "$bin" "$name")
        echo "$side $name $((total - setup))" >>"$work/counts"
    done
done

awk -v t="$threshold" '
    { key = $2; if (!(key in base)) order[n++] = key
      if ($1 == "base") base[key] = $3; else head[key] = $3 }
    END {
        printf "%-20s %14s %14s %8s\n", "workload", "base instr", "head instr", "change"
        fail = 0
        for (i = 0; i < n; i++) {
            k = order[i]; d = (head[k] - base[k]) * 100 / base[k]
            mark = d > t ? "  <- more instructions" : ""
            if (d > t) fail = 1
            printf "%-20s %14d %14d %+7.2f%%%s\n", k, base[k], head[k], d, mark
        }
        exit fail
    }' "$work/counts"

#!/bin/sh
# Fails when the working tree is more than THRESHOLD percent slower than BASE on any
# workload of benches/gate.rs.
#
#   scripts/perf-gate.sh [BASE] [THRESHOLD]      (defaults: origin/main, 10)
#
# Both builds run the same gate.rs, in alternating rounds on the same machine, and each
# workload keeps its fastest time per build, so a noisy neighbour hurts both sides alike.
set -eu
base=${1:-origin/main}
threshold=${2:-10}
rounds=${ROUNDS:-5}
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

i=0
while [ "$i" -lt "$rounds" ]; do
    "$base_bin" | sed 's/^/base /' >>"$work/times"
    "$head_bin" | sed 's/^/head /' >>"$work/times"
    i=$((i + 1))
done

awk -v t="$threshold" '
    { key = $2; if (!(key in best_base)) order[n++] = key
      if ($1 == "base" && (!(key in best_base) || $3 < best_base[key])) best_base[key] = $3
      if ($1 == "head" && (!(key in best_head) || $3 < best_head[key])) best_head[key] = $3 }
    END {
        printf "%-20s %12s %12s %8s\n", "workload", "base ns", "head ns", "change"
        fail = 0
        for (i = 0; i < n; i++) {
            k = order[i]; d = (best_head[k] - best_base[k]) * 100 / best_base[k]
            mark = d > t ? "  <- slower" : ""
            if (d > t) fail = 1
            printf "%-20s %12d %12d %+7.1f%%%s\n", k, best_base[k], best_head[k], d, mark
        }
        exit fail
    }' "$work/times"

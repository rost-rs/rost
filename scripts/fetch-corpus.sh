#!/bin/sh
# Downloads the standard JSON benchmark corpus (canada, citm_catalog, twitter) from
# serde-rs/json-benchmark into benches/data/, which git ignores.
#
#   scripts/fetch-corpus.sh && cargo bench --features serde --bench corpus
set -eu
dir="$(dirname "$0")/../benches/data"
mkdir -p "$dir"
base=https://raw.githubusercontent.com/serde-rs/json-benchmark/master/data
for name in canada citm_catalog twitter; do
    if [ ! -s "$dir/$name.json" ]; then
        curl -fsSL "$base/$name.json" -o "$dir/$name.json.part"
        mv "$dir/$name.json.part" "$dir/$name.json"
    fi
done
ls -l "$dir"

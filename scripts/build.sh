#!/bin/sh
# Build both local executables together; no system installation required.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
controls=${CONTROLS_SOURCE:-"$root/../controls"}
mkdir -p "$root/bin"
(cd "$controls" && go build -o "$root/bin/factory-runtime" ./cmd/factory-runtime)
(cd "$root" && cargo build --release)
cp "$root/target/release/fabric" "$root/bin/fabric"
printf 'Ready. From %s run: bin/fabric up fabric.example.yaml\n' "$root"

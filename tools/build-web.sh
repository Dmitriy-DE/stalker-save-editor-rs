#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/sse-target"

cd "$repo_root"
cargo build --release --target wasm32-unknown-unknown -p sse-sys
wasm_file="$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/sse_sys.wasm"
wasm_bytes=$(wc -c < "$wasm_file")
if [ "$wasm_bytes" -ge 1048576 ]; then
    printf 'WebAssembly output is %s bytes; limit is less than 1048576 bytes\n' "$wasm_bytes" >&2
    exit 1
fi
cp "$wasm_file" "$repo_root/web/sse_sys.wasm"
printf 'Browser shell: %s bytes (%s)\n' "$wasm_bytes" "$repo_root/web/sse_sys.wasm"

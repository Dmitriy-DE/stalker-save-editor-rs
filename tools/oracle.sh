#!/usr/bin/env bash
# oracle.sh [SAVE...]: prints nothing when `stalker-save` answers exactly like the released C# command line.
# Without arguments it walks fixtures/synthetic. SSE_ORACLE is the C# CLI (stalker-save-editor-cli of release 1.3.1),
# SSE_NEW the Rust one (default: target/release/stalker-save). Saves are only read.
set -u
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
oracle="${SSE_ORACLE:?set SSE_ORACLE to the C# stalker-save-editor-cli}"
new="${SSE_NEW:-$root/target/release/stalker-save}"
if [ "$#" -eq 0 ]; then mapfile -t saves < <(find "$root/fixtures/synthetic" -type f \( -name '*.sav' -o -name '*.scop' -o -name '*.scoc' \) | sort); else saves=("$@"); fi
failed=0
for save in "${saves[@]}"; do
  for command in info inventory; do
    want="$("$oracle" "$command" "$save" 2>&1)"; want_code=$?
    got="$("$new" "$command" "$save" 2>&1)"; got_code=$?
    if [ "$want" != "$got" ] || [ "$want_code" != "$got_code" ]; then
      failed=$((failed + 1))
      echo "DIFF $command $save (exit $want_code vs $got_code)"
      diff <(echo "$want") <(echo "$got") | head -5
    fi
  done
done
echo "${#saves[@]} saves, $failed differences"
[ "$failed" -eq 0 ]

#!/usr/bin/env bash
# With no arguments, compares fixture reads and runs Rust money/stack writes into a temporary directory before
# asking the C# 1.3.1 CLI to read them. With SAVE arguments, performs read-only parity checks for those paths.
# SSE_ORACLE is the C# CLI; SSE_NEW is the Rust CLI (default: $CARGO_TARGET_DIR/release/stalker-save).
set -u
set -o pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cache_home="${HOME:-}"
if [[ -z "$cache_home" ]]; then
  printf 'Set HOME or CARGO_TARGET_DIR before running the oracle.\n' >&2
  exit 2
fi
target_dir="${CARGO_TARGET_DIR:-$cache_home/.cache/sse-target}"
oracle="${SSE_ORACLE:-}"
new="${SSE_NEW:-$target_dir/release/stalker-save}"
if [[ -z "$oracle" || ! -x "$oracle" ]]; then
  printf 'Set SSE_ORACLE to an executable C# 1.3.1 stalker-save-editor-cli.\n' >&2
  exit 2
fi
if [[ ! -x "$new" ]]; then
  printf 'Rust CLI not found at %s; build it with CARGO_TARGET_DIR=%s cargo build --release -p sse-cli.\n' "$new" "$target_dir" >&2
  exit 2
fi
if ! oracle_version="$("$oracle" version 2>&1)"; then
  printf 'Cannot read C# oracle version: %s\n' "$oracle_version" >&2
  exit 2
fi
if [[ "$oracle_version" != "1.3.1" ]]; then
  printf 'C# oracle must be version 1.3.1; found: %s\n' "$oracle_version" >&2
  exit 2
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/sse-oracle.XXXXXX")" || exit 2
trap 'rm -rf "$work"' EXIT

read_failures=0
write_failures=0
write_cases=0
capture_status=0

capture() {
  local output="$1"
  shift
  if "$@" >"$output" 2>&1; then
    capture_status=0
  else
    capture_status=$?
  fi
}

compare_reads() {
  local save="$1"
  local label="$2"
  local command_name
  local wanted="$work/$label.csharp"
  local actual="$work/$label.rust"
  local wanted_code
  local actual_code
  local equal=1

  for command_name in info inventory; do
    capture "$wanted" "$oracle" "$command_name" "$save"
    wanted_code="$capture_status"
    capture "$actual" "$new" "$command_name" "$save"
    actual_code="$capture_status"
    if [[ "$wanted_code" != "$actual_code" ]] || ! cmp -s "$wanted" "$actual"; then
      printf 'DIFF %s %s (C# exit %s, Rust exit %s)\n' "$command_name" "$save" "$wanted_code" "$actual_code"
      diff -u "$wanted" "$actual" || true
      equal=0
    fi
  done

  if [[ "$equal" -eq 0 ]]; then
    read_failures=$((read_failures + 1))
    return 1
  fi
  return 0
}

normalize_info() {
  local input="$1"
  local output="$2"
  local edit_kind="$3"
  if [[ "$edit_kind" == money ]]; then
    sed -E '/^(Packed|Raw|SHA256|CRC|Money):[[:space:]]*/d' "$input" >"$output"
  else
    sed -E '/^(Packed|Raw|SHA256|CRC):[[:space:]]*/d' "$input" >"$output"
  fi
}

normalize_inventory_count() {
  local input="$1"
  local handle="$2"
  local output="$3"
  awk -v handle="$handle" '
    {
      if (tolower($NF) == tolower(handle)) {
        if (NF < 2) bad = 1
        $(NF - 1) = "<COUNT>"
        found++
      }
      print
    }
    END { if (found != 1 || bad) exit 1 }
  ' "$input" >"$output"
}

assert_inventory_count() {
  local input="$1"
  local handle="$2"
  local expected="$3"
  awk -v handle="$handle" -v expected="$expected" '
    tolower($NF) == tolower(handle) {
      found++
      if (NF < 2 || $(NF - 1) != expected) bad = 1
    }
    END { if (found != 1 || bad) exit 1 }
  ' "$input"
}

compare_normalized() {
  local before="$1"
  local after="$2"
  local description="$3"
  if ! cmp -s "$before" "$after"; then
    printf 'UNEXPECTED CHANGE: %s\n' "$description"
    diff -u "$before" "$after" || true
    write_failures=$((write_failures + 1))
    return 1
  fi
  return 0
}

run_write_case() {
  local edit_kind="$1"
  local source="$2"
  local handle_or_value="$3"
  local expected_value="$4"
  local label="$5"
  local expected_bytes="${6:-}"
  local output="$work/$label.sav"
  local before_info="$work/$label.before.info"
  local before_inventory="$work/$label.before.inventory"
  local after_info="$work/$label.after.info"
  local after_inventory="$work/$label.after.inventory"
  local rust_log="$work/$label.write.log"
  local expected_handle=""

  write_cases=$((write_cases + 1))
  if [[ "$edit_kind" == money ]]; then
    "$new" set-money "$source" "$handle_or_value" --output "$output" --backup-dir "$work/backups" >"$rust_log" 2>&1
  else
    expected_handle="$handle_or_value"
    "$new" set-stack "$source" "$handle_or_value" "$expected_value" --output "$output" --backup-dir "$work/backups" >"$rust_log" 2>&1
  fi
  if [[ "$?" -ne 0 || ! -f "$output" ]]; then
    printf 'WRITE FAILED: %s %s\n' "$edit_kind" "$source"
    cat "$rust_log"
    write_failures=$((write_failures + 1))
    return 1
  fi

  # Byte-exact check against the committed expected image; the C# read-back below only sees the text view.
  if [[ -n "$expected_bytes" ]] && ! cmp -s "$output" "$expected_bytes"; then
    printf 'BYTES DIFFER: %s output is not the committed expected image %s\n' "$label" "$expected_bytes"
    write_failures=$((write_failures + 1))
  fi

  capture "$before_info" "$oracle" info "$source"
  if [[ "$capture_status" -ne 0 ]]; then
    printf 'C# could not read source info for %s (exit %s)\n' "$source" "$capture_status"
    cat "$before_info"
    write_failures=$((write_failures + 1))
    return 1
  fi
  capture "$before_inventory" "$oracle" inventory "$source"
  if [[ "$capture_status" -ne 0 ]]; then
    printf 'C# could not read source inventory for %s (exit %s)\n' "$source" "$capture_status"
    cat "$before_inventory"
    write_failures=$((write_failures + 1))
    return 1
  fi
  capture "$after_info" "$oracle" info "$output"
  if [[ "$capture_status" -ne 0 ]]; then
    printf 'C# could not read Rust output info for %s (exit %s)\n' "$source" "$capture_status"
    cat "$after_info"
    write_failures=$((write_failures + 1))
    return 1
  fi
  capture "$after_inventory" "$oracle" inventory "$output"
  if [[ "$capture_status" -ne 0 ]]; then
    printf 'C# could not read Rust output inventory for %s (exit %s)\n' "$source" "$capture_status"
    cat "$after_inventory"
    write_failures=$((write_failures + 1))
    return 1
  fi

  if [[ "$edit_kind" == money ]]; then
    if ! grep -Fqx "Money: $handle_or_value" "$after_info"; then
      printf 'C# read unexpected money after editing %s\n' "$source"
      write_failures=$((write_failures + 1))
    fi
    normalize_info "$before_info" "$work/$label.before.normalized.info" money
    normalize_info "$after_info" "$work/$label.after.normalized.info" money
    compare_normalized "$work/$label.before.normalized.info" "$work/$label.after.normalized.info" "$source info except money and derived size/hash/CRC"
    compare_normalized "$before_inventory" "$after_inventory" "$source inventory after money edit"
  else
    if ! assert_inventory_count "$after_inventory" "$expected_handle" "$expected_value"; then
      printf 'C# read unexpected count for handle %s after editing %s\n' "$expected_handle" "$source"
      write_failures=$((write_failures + 1))
    fi
    normalize_info "$before_info" "$work/$label.before.normalized.info" stack
    normalize_info "$after_info" "$work/$label.after.normalized.info" stack
    compare_normalized "$work/$label.before.normalized.info" "$work/$label.after.normalized.info" "$source info except derived size/hash/CRC"
    if normalize_inventory_count "$before_inventory" "$expected_handle" "$work/$label.before.normalized.inventory" \
      && normalize_inventory_count "$after_inventory" "$expected_handle" "$work/$label.after.normalized.inventory"; then
      compare_normalized "$work/$label.before.normalized.inventory" "$work/$label.after.normalized.inventory" "$source inventory except handle $expected_handle count"
    else
      printf 'C# inventory did not contain exactly one row for handle %s in %s\n' "$expected_handle" "$source"
      write_failures=$((write_failures + 1))
    fi
  fi

  if ! compare_reads "$output" "$label.after-parity"; then
    write_failures=$((write_failures + 1))
  fi
  return 0
}

run_fixture_writes() {
  local game
  for game in soc cs cop soc-ee cs-ee cop-ee; do
    run_write_case money "$root/fixtures/synthetic/writer-money/xray-money-$game-source.sav" 876543 '' "xray-money-$game" \
      "$root/fixtures/synthetic/writer-money/xray-money-$game-expected.sav"
  done
  for game in soc cs cop soc-ee cs-ee cop-ee; do
    run_write_case stack "$root/fixtures/synthetic/writer-stacks/xray-stack-$game-source.sav" 0x1234 44 "xray-stack-$game" \
      "$root/fixtures/synthetic/writer-stacks/xray-stack-$game-expected.sav"
  done
  run_write_case money "$root/fixtures/synthetic/writer-s2-money/s2-money-source.sav" 876543 '' s2-money
  run_write_case stack "$root/fixtures/synthetic/writer-s2-stacks/s2-stacks-source.sav" 0x30000001 7 s2-stack
}

saves=()
run_fixture_writes_by_default=0
if [[ "$#" -eq 0 ]]; then
  run_fixture_writes_by_default=1
  while IFS= read -r save; do
    saves+=("$save")
  done < <(find "$root/fixtures/synthetic" -type f \( -name '*.sav' -o -name '*.scop' -o -name '*.scoc' \) | sort)
else
  saves=("$@")
fi

if [[ "${#saves[@]}" -eq 0 ]]; then
  printf 'No save fixtures or paths were selected.\n' >&2
  exit 2
fi

index=0
for save in "${saves[@]}"; do
  index=$((index + 1))
  compare_reads "$save" "read-$index" || true
done

if [[ "$run_fixture_writes_by_default" -eq 1 ]]; then
  run_fixture_writes
fi

if [[ "$run_fixture_writes_by_default" -eq 1 ]]; then
  printf '%s saves, %s read differences; %s writes, %s write failures\n' \
    "${#saves[@]}" "$read_failures" "$write_cases" "$write_failures"
else
  printf '%s saves, %s read differences; writer fixtures skipped for explicit paths\n' "${#saves[@]}" "$read_failures"
fi

if [[ "$read_failures" -ne 0 || "$write_failures" -ne 0 ]]; then
  exit 1
fi

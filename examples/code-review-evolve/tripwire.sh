#!/usr/bin/env bash
# The Self-Modification Tripwire.
#
# Demo for the "skill-audit cron self-improves a skill" workflow
# (user stories #56, #66, #152). Runs in under 30 seconds. Shows both
# outcomes the operator needs to see:
#
#   1. Good mutation (v0.1.0 -> v0.1.1, adds `category`):
#      lineage receipt MINTS. Promotion is safe.
#
#   2. Bad mutation (v0.1.0 -> v0.2.0-bad, drops `severity`):
#      lineage receipt FAILS with R4 OutputWidened. Promotion is rolled back.
#
# Usage: ./tripwire.sh [path-to-lyra-binary]
#
# Default binary path assumes you've run `cargo build --release` in the
# repo root.

set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
LYRA_IN="${1:-../../target/release/lyra}"
# Resolve to absolute path. Relative paths are interpreted relative to the
# script's own directory (HERE), not the caller's CWD, so the default works
# no matter where the user invokes the script from.
case "$LYRA_IN" in
  /*) LYRA="$LYRA_IN" ;;
  *)  LYRA="$(cd "$HERE/$(dirname "$LYRA_IN")" 2>/dev/null && pwd)/$(basename "$LYRA_IN")" ;;
esac
cd "$HERE"

if [ ! -x "$LYRA" ] && [ ! -x "${LYRA}.exe" ]; then
  echo "lyra binary not found at: $LYRA"
  echo "build it first: cargo build --release"
  exit 2
fi
[ -x "${LYRA}.exe" ] && LYRA="${LYRA}.exe"

GREEN=$'\033[32m'; RED=$'\033[31m'; DIM=$'\033[2m'; BOLD=$'\033[1m'; OFF=$'\033[0m'

printf '\n%s== Lyra self-modification tripwire ==%s\n' "$BOLD" "$OFF"
printf '%sHermes cron tries to promote two child versions of `code-review-evolve`.%s\n' "$DIM" "$OFF"
printf '%sOnly the legitimate refinement is allowed to ship.%s\n\n' "$DIM" "$OFF"

# 1. Mint the parent receipt. This is what the running production skill carries.
printf '%s[1/3]%s mint parent receipt (v0.1.0, in production)\n' "$BOLD" "$OFF"
"$LYRA" score skill_interface_hash "$(tr -d '\n' < v0.1.0.lyra.json)" \
  ./_parent_receipt.json >/dev/null
PARENT_HASH=$(grep -o '"output_hash":"[a-f0-9]*"' ./_parent_receipt.json | cut -d'"' -f4)
printf '  parent output_hash = %s%s...%s\n' "$DIM" "${PARENT_HASH:0:16}" "$OFF"
PR_B64=$(base64 -w0 < ./_parent_receipt.json 2>/dev/null || base64 < ./_parent_receipt.json | tr -d '\n')

# 2. Good case: v0.1.1 adds `category`. R4 holds (child output >= parent).
printf '\n%s[2/3]%s cron proposes v0.1.1 (adds `category` field)\n' "$BOLD" "$OFF"
CHILD_OK=$(tr -d '\n' < v0.1.1.lyra.json)
if "$LYRA" score next_generation \
   "{\"parent_receipt\":\"$PR_B64\",\"child_descriptor\":$CHILD_OK}" \
   ./_lineage_ok.json >/dev/null 2>&1
then
   OK_HASH=$(grep -o '"output_hash":"[a-f0-9]*"' ./_lineage_ok.json | cut -d'"' -f4)
   printf '  %sOK lineage receipt minted%s = %s%s...%s\n' "$GREEN" "$OFF" "$DIM" "${OK_HASH:0:16}" "$OFF"
   printf '  %sPROMOTE%s v0.1.1 to production.\n' "$GREEN" "$OFF"
else
   printf '  %sunexpected failure on a legitimate refinement%s\n' "$RED" "$OFF"
   exit 2
fi

# 3. Bad case: v0.2.0 drops `severity`. R4 fails -> OutputWidened.
printf '\n%s[3/3]%s cron proposes v0.2.0-bad (drops `severity` to save tokens)\n' "$BOLD" "$OFF"
CHILD_BAD=$(tr -d '\n' < v0.2.0-bad.lyra.json)
ERR=$("$LYRA" score next_generation \
     "{\"parent_receipt\":\"$PR_B64\",\"child_descriptor\":$CHILD_BAD}" \
     ./_lineage_bad.json 2>&1) && {
   printf '  %sFAIL a regression was just promoted; tripwire did not fire%s\n' "$RED" "$OFF"
   exit 1
}
SHORT_ERR=$(printf '%s' "$ERR" | tr '\n' ' ' | head -c 200)
printf '  %slineage rejected%s: %s\n' "$RED" "$OFF" "$SHORT_ERR"
printf '  %sROLLBACK%s. v0.2.0-bad stays in staging. Operator paged.\n' "$RED" "$OFF"

printf '\n%s== done. ==%s both outcomes are deterministic and replayable on any machine\n' "$BOLD" "$OFF"
printf '%swith the same hermes-lyra + uor-foundation substrate.%s\n\n' "$DIM" "$OFF"

rm -f ./_parent_receipt.json ./_lineage_ok.json ./_lineage_bad.json 2>/dev/null || true

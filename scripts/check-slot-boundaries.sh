#!/bin/sh
# Check the function musicindex_boundaries of the slot text against the shared segment cases
# (ADR 0045 §Verification). The build host has no Liquidsoap, so run this script on a host
# or in a container that has Liquidsoap, for example the AzuraCast container of the test station.
#
# Usage: check-slot-boundaries.sh SLOT_TEXT CASES
#   SLOT_TEXT  liquidsoap/musicindex-state.liq
#   CASES      liquidsoap-now-playing/tests/segment-boundaries.json
# Exit 0 when each case passes, 1 when a case fails, 2 for a usage or setup error.
set -eu

if [ "$#" -ne 2 ]; then
    echo "usage: $0 SLOT_TEXT CASES" >&2
    exit 2
fi
slot_text=$1
cases=$2
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

sed -n '/^# BEGIN musicindex_boundaries$/,/^# END musicindex_boundaries$/p' "$slot_text" > "$work/check.liq"
if ! grep -q '^def musicindex_boundaries' "$work/check.liq"; then
    echo "ADR 0045 §Verification: $slot_text has no block '# BEGIN musicindex_boundaries' to '# END musicindex_boundaries'." >&2
    echo "Keep the function between the two marker lines, so that this check can read it." >&2
    exit 2
fi
cp "$cases" "$work/cases.json"

cat >> "$work/check.liq" <<'EOF'

let json.parse (musicindex_cases : [{name: string, chapters: string, boundaries: [int], suffix: bool}]) =
  file.contents("cases.json")
musicindex_failed = ref(0)
def musicindex_check(c) =
  let (suffix, bounds) = musicindex_boundaries(c.chapters)
  got = list.sort(fun (a, b) -> a - b, list.append([0], bounds))
  if got == c.boundaries and suffix == c.suffix then
    print("PASS #{c.name}")
  else
    musicindex_failed := musicindex_failed() + 1
    print("FAIL #{c.name}: expected #{c.boundaries} suffix #{c.suffix}, got #{got} suffix #{suffix}")
  end
end
list.iter(musicindex_check, musicindex_cases)
print("RESULT #{list.length(musicindex_cases)} cases, #{musicindex_failed()} failed")
EOF

cd "$work"
# The script has no output, so Liquidsoap stops with "No output defined" and a status that is not 0.
# The RESULT line tells that each case ran.
output=$(liquidsoap check.liq 2>&1 || true)
if ! echo "$output" | grep -q '^RESULT '; then
    echo "$output" >&2
    echo "ADR 0045 §Verification: Liquidsoap did not run the check script." >&2
    exit 2
fi
echo "$output" | grep -E '^(PASS|FAIL|RESULT) '
if ! echo "$output" | grep -q '^RESULT .* 0 failed$'; then
    echo "ADR 0045 §The Segment Rule: the slot text and the shared cases disagree." >&2
    echo "Change musicindex_boundaries in the slot text, or change the rule in ADR 0045 and the cases together." >&2
    exit 1
fi

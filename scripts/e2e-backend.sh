#!/usr/bin/env bash
# Phase 10 — prove the backend as one system: core + SQLite + FSRS + selection
# + D-Bus + CLI. Runs on a private session bus against a throwaway database, so
# it never touches the user's real vocabulary.
#
#   ./scripts/e2e-backend.sh [path-to-target-dir]

set -euo pipefail

TARGET="${1:-$(cd "$(dirname "$0")/.." && pwd)/target/debug}"
SERVICE="$TARGET/wordbar-service"
CTL="$TARGET/wordbarctl"

for binary in "$SERVICE" "$CTL"; do
    [[ -x "$binary" ]] || { echo "missing $binary — run: cargo build" >&2; exit 1; }
done

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# D-Bus activation: the service must be started by the bus, never by us.
#
# The bus launches it with dbus-daemon's stdout, and the service deliberately
# outlives each request, so it would hold this script's pipe open forever.
# A wrapper redirects its output to a log and records its pid for the restart
# check below.
mkdir -p "$WORK/dbus-1/services"
cat > "$WORK/launch-service" <<EOF
#!/usr/bin/env bash
echo \$\$ > "$WORK/service.pid"
exec "$SERVICE" >> "$WORK/service.log" 2>&1
EOF
chmod +x "$WORK/launch-service"

cat > "$WORK/dbus-1/services/org.wordbar.Wordbar.service" <<EOF
[D-BUS Service]
Name=org.wordbar.Wordbar
Exec=$WORK/launch-service
EOF

export XDG_DATA_DIRS="$WORK:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
export WORDBAR_DATA_DIR="$WORK/db"
export WORDBAR_DICTIONARY=off      # offline: enrichment must be optional
export WORDBAR_LOG="${WORDBAR_LOG:-warn}"

pass=0
fail=0

check() { # check <description> <expected> <actual>
    if [[ "$2" == "$3" ]]; then
        printf '  ok    %s\n' "$1"; pass=$((pass + 1))
    else
        printf '  FAIL  %s\n         expected: %s\n         actual:   %s\n' "$1" "$2" "$3"
        fail=$((fail + 1))
    fi
}

check_contains() {
    if [[ "$3" == *"$2"* ]]; then
        printf '  ok    %s\n' "$1"; pass=$((pass + 1))
    else
        printf '  FAIL  %s\n         expected to contain: %s\n         actual: %s\n' "$1" "$2" "$3"
        fail=$((fail + 1))
    fi
}

status() { set +e; "$@" >/dev/null 2>&1; local rc=$?; set -e; echo "$rc"; }

run_suite() {
    echo "activation"
    check_contains "the bus starts the service on the first call" "wordbar-service" "$($CTL ping)"
    check_contains "the database was created" "wordbar.db" "$(ls "$WORDBAR_DATA_DIR")"

    echo "empty vocabulary"
    check "current exits 2 when there is nothing to show" "2" "$(status $CTL current)"
    check "waybar output still exits 0" "0" "$(status $CTL current --waybar)"
    check_contains "waybar reports the empty state" '"class":"empty"' "$($CTL current --waybar)"

    echo "add"
    check_contains "add reports the term" "Added" "$($CTL add ephemeral)"
    check "adding a duplicate exits 3" "3" "$(status $CTL add Ephemeral)"
    check_contains "the duplicate message names the word" "already in your vocabulary" \
        "$($CTL add EPHEMERAL 2>&1 || true)"
    check "a five-word phrase is accepted" "0" "$(status $CTL add by and large)"
    check "a six-word phrase is rejected" "1" "$(status $CTL add one two three four five six)"

    echo "current is read-only"
    first="$($CTL current --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["term"])')"
    for _ in 1 2 3 4 5; do
        again="$($CTL current --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["term"])')"
        [[ "$again" == "$first" ]] || break
    done
    check "polling current never rotates" "$first" "$again"

    echo "next changes state"
    $CTL next >/dev/null
    second="$($CTL current --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["term"])')"
    check_contains "next moved to the other word" "1" "$([[ "$second" != "$first" ]] && echo 1 || echo 0)"

    echo "ratings"
    check_contains "again advances" "Next:" "$($CTL again)"
    check_contains "know advances" "Next:" "$($CTL know)"
    check "rating an unknown term exits 2" "2" "$(status $CTL know nonexistent)"

    echo "learned and restore"
    check_contains "learned confirms" "as learned" "$($CTL learned ephemeral)"
    check_contains "a learned word is listed as known" "ephemeral" "$($CTL list --known)"
    check "it no longer appears among words being learned" "0" \
        "$($CTL list | grep -c '^ephemeral' || true)"
    check_contains "restore confirms" "Restored" "$($CTL restore ephemeral)"
    check_contains "it is active again" "ephemeral" "$($CTL list)"

    echo "waybar output"
    waybar="$($CTL current --waybar)"
    check_contains "waybar json has text" '"text"' "$waybar"
    check_contains "waybar json has tooltip" '"tooltip"' "$waybar"
    check_contains "waybar json has class" '"class"' "$waybar"
    check "waybar output is one line" "1" "$(printf '%s' "$waybar" | wc -l | awk '{print $1+1}')"
    check "waybar output is valid json" "0" "$(status python3 -c "import json,sys; json.loads(sys.argv[1])" "$waybar")"

    echo "import and export"
    printf 'ubiquitous,present or found everywhere\nlaconic\n' > "$WORK/in.csv"
    check_contains "csv import reports counts" "Imported 2" "$($CTL import "$WORK/in.csv")"
    check_contains "re-importing skips duplicates" "skipped 2 duplicate" \
        "$($CTL import "$WORK/in.csv")"
    exported="$($CTL export --all)"
    check "export is valid json" "0" "$(status python3 -c "import json,sys; json.loads(sys.argv[1])" "$exported")"
    check_contains "export carries definitions" "present or found everywhere" "$exported"
    check "export has no scheduler internals" "0" \
        "$(printf '%s' "$exported" | grep -c -e stability -e difficulty || true)"

    echo "stats and delete"
    check_contains "stats prints counts" "active" "$($CTL stats)"
    check_contains "delete confirms" "Deleted" "$($CTL delete laconic)"
    check "deleting it again exits 2" "2" "$(status $CTL delete laconic)"

    echo "restart resilience"
    # Kill only the service this run started, never a real one on the user bus.
    kill "$(cat "$WORK/service.pid")" 2>/dev/null || true
    sleep 0.5
    check_contains "the bus re-activates the service" "wordbar-service" "$($CTL ping)"
    check_contains "the vocabulary survived" "ubiquitous" "$($CTL list)"

    # Leave no service behind on the private bus.
    kill "$(cat "$WORK/service.pid")" 2>/dev/null || true

    echo
    echo "$pass passed, $fail failed"
    [[ $fail -eq 0 ]]
}

export -f run_suite check check_contains status
export CTL SERVICE WORK pass fail

dbus-run-session -- bash -c run_suite

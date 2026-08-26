#!/usr/bin/env bash
# Prove that a failed dictionary lookup is retried, and that a lookup which
# succeeded in saying "no such word" is not.
#
# Separate from e2e-backend.sh, which runs with enrichment off so it can
# guarantee the backend works offline. This one needs a dictionary, so it
# serves its own: a stub that fails the first request for a term and answers
# the second.
#
# Runs on a private session bus against a throwaway database.
#
#   ./scripts/e2e-retry.sh [path-to-target-dir]

set -euo pipefail

TARGET="${1:-$(cd "$(dirname "$0")/.." && pwd)/target/debug}"
SERVICE="$TARGET/wordbar-service"
CTL="$TARGET/wordbarctl"

for binary in "$SERVICE" "$CTL"; do
    [[ -x "$binary" ]] || { echo "missing $binary — run: cargo build" >&2; exit 1; }
done

WORK="$(mktemp -d)"
STUB_PID=""
cleanup() {
    [[ -n "$STUB_PID" ]] && kill "$STUB_PID" 2>/dev/null || true
    [[ -f "$WORK/service.pid" ]] && kill "$(cat "$WORK/service.pid")" 2>/dev/null || true
    rm -rf "$WORK"
}
trap cleanup EXIT

# -- the stub dictionary ---------------------------------------------------
#
# `flaky` fails once with a 503 and then answers. `missing` always 404s, which
# the provider reports as "no entry" rather than an error. Every request is
# appended to a log so the test can count attempts.

cat > "$WORK/stub.py" <<'PY'
import json, sys
from http.server import BaseHTTPRequestHandler, HTTPServer

LOG = sys.argv[2]
seen = {}

ENTRY = [{
    "word": "flaky",
    "phonetic": "/fleɪki/",
    "phonetics": [],
    "meanings": [{
        "partOfSpeech": "adjective",
        "definitions": [{"definition": "Came back after a retry."}],
    }],
}]


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        term = self.path.rsplit("/", 1)[-1]
        seen[term] = seen.get(term, 0) + 1
        with open(LOG, "a") as f:
            f.write(f"{term} {seen[term]}\n")

        if term == "missing":
            self.send_error(404)
            return
        if seen[term] == 1:
            self.send_error(503)
            return

        body = json.dumps(ENTRY).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


HTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
PY

PORT="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')"
touch "$WORK/requests.log"
python3 "$WORK/stub.py" "$PORT" "$WORK/requests.log" &
STUB_PID=$!

for _ in $(seq 50); do
    curl -sf -o /dev/null "http://127.0.0.1:$PORT/absent" && break
    sleep 0.1
done

# -- the service ----------------------------------------------------------

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
# Pin the primary provider: the real fallback would answer every one of these
# lookups and there would be no failure left to retry.
export WORDBAR_DICTIONARY=primary
export WORDBAR_DICTIONARY_ENDPOINT="http://127.0.0.1:$PORT"
export WORDBAR_LOG="${WORDBAR_LOG:-info}"

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

# The first backoff step is 15s, so allow generous headroom without making the
# whole script wait when it works.
await_definition() { # await_definition <term> <seconds>
    local term="$1" deadline=$((SECONDS + $2))
    while (( SECONDS < deadline )); do
        if [[ -n "$($CTL export --all --format csv | awk -F, -v t="$term" '$1 == t {print $2}')" ]]; then
            echo yes; return
        fi
        sleep 1
    done
    echo no
}

attempts() { # attempts <term>
    grep -c "^$1 " "$WORK/requests.log" || true
}

run_suite() {
    echo "a lookup that failed is retried"
    $CTL add flaky > /dev/null
    # Give the first attempt time to fail before asserting it did.
    sleep 3
    check "the first attempt failed, so the word has no definition yet" \
        "" "$($CTL export --all --format csv | awk -F, '$1 == "flaky" {print $2}')"
    check "the stub was asked exactly once" "1" "$(attempts flaky)"
    check "the retry filled the definition in" "yes" "$(await_definition flaky 40)"
    check "and it took a second request to do it" "2" "$(attempts flaky)"

    echo
    echo "a word the dictionary does not have is not retried"
    $CTL add missing > /dev/null
    check "the definition never arrives" "no" "$(await_definition missing 25)"
    check "and the stub was asked only once" "1" "$(attempts missing)"

    echo
    echo "correcting the spelling asks again under the new term"
    # Renaming is manager-only, so go through the interface the manager uses.
    local id
    id="$($CTL list --all --json | python3 -c \
        'import json,sys; print(next(w["id"] for w in json.load(sys.stdin) if w["term"] == "missing"))')"
    gdbus call --session --dest org.wordbar.Wordbar \
        --object-path /org/wordbar/Wordbar \
        --method org.wordbar.Wordbar.UpdateWord "$id" "{'term': 'corrected'}" > /dev/null
    check "the corrected spelling gets looked up" "yes" "$(await_definition corrected 40)"
}

dbus-run-session -- bash -c "$(declare -f check await_definition attempts run_suite); \
    CTL=$CTL WORK=$WORK SECONDS=0; run_suite; echo \"\$pass \$fail\" > $WORK/tally"

read -r pass fail < "$WORK/tally"
echo
if (( fail > 0 )); then
    echo "$pass passed, $fail failed"
    echo "-- service log --"
    tail -30 "$WORK/service.log" || true
    exit 1
fi
echo "$pass checks passed"

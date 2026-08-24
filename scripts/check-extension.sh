#!/usr/bin/env bash
# Static checks for the GNOME Shell extension.
#
# GNOME Shell loads each extension module exactly once per session, so a fault
# during module evaluation costs a full logout to diagnose and another to
# retest. These checks catch that class of bug before installing.

set -euo pipefail

DIR="${1:-$(cd "$(dirname "$0")/.." && pwd)/extension}"
failures=0

fail() { printf '  FAIL  %s\n' "$1"; failures=$((failures + 1)); }
pass() { printf '  ok    %s\n' "$1"; }

echo "metadata"
if python3 -c "
import json, sys
m = json.load(open('$DIR/metadata.json'))
for key in ('uuid', 'name', 'description', 'shell-version'):
    assert key in m, f'missing {key}'
assert m['uuid'].count('@') == 1, 'uuid must be name@domain'
" 2>/dev/null; then
    pass "metadata.json is valid and complete"
else
    fail "metadata.json is invalid"
fi

echo "module graph"
missing=$(cd "$DIR" && for f in *.js; do
    grep -oP "from '\./\K[^']+" "$f" 2>/dev/null | while read -r dep; do
        [ -f "$dep" ] || echo "$f -> $dep"
    done
done || true)
if [ -z "$missing" ]; then
    pass "every relative import resolves"
else
    fail "unresolved imports:
$missing"
fi

echo "module evaluation"
# gettext() identifies the calling extension from the stack, so it throws when
# called while a module is still being evaluated. Same for anything else that
# needs the extension object.
if grep -n "^\(export \)\?\(const\|let\|var\).*[^a-zA-Z]_(" "$DIR"/*.js 2>/dev/null; then
    fail "gettext called at module scope (must be lazy)"
else
    pass "no gettext at module scope"
fi

risky=$(cd "$DIR" && python3 - <<'PY'
import pathlib, re
out = []
for p in sorted(pathlib.Path('.').glob('*.js')):
    for i, line in enumerate(p.read_text().splitlines(), 1):
        if not line or line[0] in ' \t}/*':
            continue
        if line.startswith(('import ', 'export ', 'function ', 'class ', '#!')):
            continue
        m = re.match(r'(?:export\s+)?(?:const|let|var)\s+\w+\s*=\s*(.*)', line)
        init = m.group(1) if m else line
        if re.search(r'\w\s*\(', init) and not re.search(r'registerClass|makeProxyWrapper', init):
            out.append(f"{p.name}:{i}: {line.strip()[:80]}")
print("\n".join(out))
PY
)
if [ -z "$risky" ]; then
    pass "no unreviewed top-level calls"
else
    fail "top-level calls run before the extension exists:
$risky"
fi

echo "schema"
if glib-compile-schemas --strict --dry-run "$DIR/schemas" 2>/dev/null; then
    pass "GSettings schema compiles"
else
    fail "GSettings schema does not compile"
fi

echo
if [ $failures -eq 0 ]; then
    echo "extension checks passed"
else
    echo "$failures check(s) failed"
    exit 1
fi

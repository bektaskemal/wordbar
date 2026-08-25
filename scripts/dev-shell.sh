#!/usr/bin/env bash
# Reload the extension without logging out.
#
# GNOME Shell *is* the Wayland compositor, so it cannot restart in place --
# Alt+F2 "r" is X11-only -- and it imports each extension module exactly once
# per process, so disable/enable never picks up edited files. That is why a
# code change otherwise costs a full logout.
#
# This runs a second GNOME Shell nested in a window instead. Close it, re-run
# this, and your changes are live. Your real session is untouched.
#
#   ./scripts/dev-shell.sh              # throwaway vocabulary (default)
#   ./scripts/dev-shell.sh --real-data  # your actual vocabulary
#
# Quit the nested Shell by closing its window.

set -euo pipefail

cd "$(dirname "$0")/.."

REAL_DATA=0
case "${1:-}" in
    --real-data) REAL_DATA=1 ;;
    '') ;;
    *) echo "usage: $0 [--real-data]" >&2; exit 2 ;;
esac

command -v gnome-shell >/dev/null || { echo "gnome-shell not found" >&2; exit 1; }

EXTENSION_UUID="wordbar@kemalbektas.dev"

./scripts/check-extension.sh
make install-extension

# Running from the VS Code snap's terminal? Its GTK and data paths point at
# core20 libraries and stale GSettings schemas that break a host gnome-shell.
# The snap conveniently keeps the originals.
if [[ -n "${SNAP:-}" ]]; then
    echo "clearing snap environment for the host gnome-shell"
    unset LD_LIBRARY_PATH GTK_PATH GTK_MODULES GTK_EXE_PREFIX \
          GDK_PIXBUF_MODULE_FILE GSETTINGS_SCHEMA_DIR XDG_DATA_HOME
    [[ -n "${XDG_DATA_DIRS_VSCODE_SNAP_ORIG:-}" ]] &&
        export XDG_DATA_DIRS="$XDG_DATA_DIRS_VSCODE_SNAP_ORIG"
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Give the nested Shell its own settings store. Sharing the live session's
# means the dev Shell inherits whatever state the real one is in -- and the
# real Shell drops an extension to INACTIVE once its files change underneath
# it, which then silently stops the dev Shell from enabling it too.
export XDG_CONFIG_HOME="$WORK/config"
mkdir -p "$XDG_CONFIG_HOME"
gsettings set org.gnome.shell disable-user-extensions false
gsettings set org.gnome.shell enabled-extensions "['$EXTENSION_UUID']"
gsettings set org.gnome.shell disabled-extensions "[]"

if [[ $REAL_DATA -eq 0 ]]; then
    export WORDBAR_DATA_DIR="$WORK/db"
    echo "vocabulary: throwaway, in $WORDBAR_DATA_DIR"
else
    echo "vocabulary: your real one in ~/.local/share/wordbar"
fi

export WORDBAR_LOG="${WORDBAR_LOG:-info}"
export MUTTER_DEBUG_DUMMY_MODE_SPECS="${MUTTER_DEBUG_DUMMY_MODE_SPECS:-1280x800}"

# A private session bus: otherwise the nested Shell fights the real one over
# org.gnome.Shell. wordbar-service is activated on it from
# ~/.local/share/dbus-1/services, exactly as in a real session.
echo "starting nested GNOME Shell -- close its window to quit"
exec dbus-run-session -- gnome-shell --nested --wayland

#!/bin/sh
# Adds shellwing to the Linux desktop for the current user: its icon and a
# launcher pointing at a built binary, so the dock, Alt+Tab and the app grid
# show it. The window's app id ("shellwing") matches shellwing.desktop.
#
#   scripts/install-desktop.sh [path/to/shellwing]   (default: target/release/shellwing)
#   scripts/install-desktop.sh --remove
set -e
root=$(cd "$(dirname "$0")/.." && pwd)
data=${XDG_DATA_HOME:-$HOME/.local/share}
icon=$data/icons/hicolor/scalable/apps/shellwing.svg
launcher=$data/applications/shellwing.desktop

if [ "$1" = "--remove" ]; then
    rm -f "$icon" "$launcher"
    echo "removed $launcher and $icon"
else
    bin=$(readlink -f "${1:-$root/target/release/shellwing}")
    if [ ! -x "$bin" ]; then
        echo "no shellwing binary at $bin: build it first, or give its path" >&2
        exit 1
    fi
    mkdir -p "$(dirname "$icon")" "$(dirname "$launcher")"
    cp "$root/packaging/icons/shellwing.svg" "$icon"
    sed "s|^Exec=.*|Exec=$bin|" "$root/packaging/linux/shellwing.desktop" > "$launcher"
    echo "installed $launcher (runs $bin) and $icon"
fi
update-desktop-database "$data/applications" 2>/dev/null || true
gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true

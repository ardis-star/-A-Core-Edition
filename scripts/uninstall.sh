#!/bin/sh
# -A Core Edition uninstaller.
#
# Removes the `-a` command installed by scripts/install.sh, together with the
# PATH block the installer may have added to a shell configuration file.
#
#   ./scripts/uninstall.sh --user      remove the user installation
#   ./scripts/uninstall.sh --system    remove the system installation (needs root)
#
# Options: --prefix DIR, --destdir DIR, --force, --quiet, -h.

set -eu

BRAND="-A Core Edition"
PROG_NAME="-a"
RC_BEGIN="# >>> -A Core Edition >>>"
RC_END="# <<< -A Core Edition <<<"

MODE="user"
PREFIX=""
DESTDIR="${DESTDIR:-}"
FORCE=0
QUIET=0

say() {
    [ "$QUIET" -eq 1 ] || printf '%s\n' "$*"
}

warn() {
    printf '%s: %s\n' "$BRAND" "$*" >&2
}

die() {
    printf '%s: error: %s\n' "$BRAND" "$*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
-A Core Edition uninstaller

USAGE:
    ./scripts/uninstall.sh [--user|--system] [options]

MODES:
    --user            remove the current user's installation (default)
    --system          remove the system-wide installation (needs root)

OPTIONS:
    --prefix DIR      look under DIR/bin instead of the default prefix
    --destdir DIR     look under DIR for a staged installation
    --force           remove a file named -a even if it is not -A Core Edition
    --quiet           only report problems
    -h, --help        show this help
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --user) MODE="user" ;;
        --system) MODE="system" ;;
        --prefix)
            [ $# -ge 2 ] || die "--prefix needs a directory"
            PREFIX="$2"
            shift
            ;;
        --destdir)
            [ $# -ge 2 ] || die "--destdir needs a directory"
            DESTDIR="$2"
            shift
            ;;
        --force) FORCE=1 ;;
        --quiet) QUIET=1 ;;
        -h|--help) usage; exit 0 ;;
        *) usage >&2; die "unknown option '$1'" ;;
    esac
    shift
done

if [ -n "$PREFIX" ]; then
    BASE_PREFIX="$PREFIX"
elif [ "$MODE" = "system" ]; then
    BASE_PREFIX="/usr/local"
else
    [ -n "${HOME:-}" ] || die "HOME is not set; cannot choose a user install directory"
    BASE_PREFIX="$HOME/.local"
fi
INSTALL_DIR="${DESTDIR}${BASE_PREFIX}/bin"
TARGET="${INSTALL_DIR}/${PROG_NAME}"
PATH_DIR="${BASE_PREFIX}/bin"

if [ "$MODE" = "system" ] && [ -z "$DESTDIR" ] && [ "$(id -u)" != "0" ]; then
    if [ "${A_ELEVATED:-0}" = "1" ]; then
        die "still not root after elevation; run this script with sudo"
    fi
    if command -v sudo >/dev/null 2>&1; then
        say "$BRAND: removing a system installation needs root, re-running with sudo"
        exec sudo env A_ELEVATED=1 "$0" "$@"
    fi
    die "removing a system installation needs root: re-run with sudo, or use --user"
fi

# ------------------------------------------------------------ remove binary
if [ ! -e "$TARGET" ]; then
    say "$BRAND: nothing to remove at $TARGET"
else
    version=$("$TARGET" --version 2>/dev/null || true)
    case "$version" in
        *"$BRAND"*)
            rm -f "$TARGET" || die "cannot remove '$TARGET'"
            say "$BRAND: removed $TARGET"
            ;;
        *)
            if [ "$FORCE" -eq 1 ]; then
                warn "'$TARGET' is not $BRAND; removing it because --force was given"
                rm -f "$TARGET" || die "cannot remove '$TARGET'"
            else
                die "'$TARGET' is not $BRAND; refusing to remove it (pass --force to override)"
            fi
            ;;
    esac
fi

# -------------------------------------------------------------- PATH cleanup
remove_path_block() {
    rc_file="$1"
    [ -f "$rc_file" ] || return 0
    grep -Fq "$RC_BEGIN" "$rc_file" || return 0
    tmp="$rc_file.a-uninstall.$$"
    if awk -v begin="$RC_BEGIN" -v end="$RC_END" '
        $0 == begin { skip = 1; next }
        $0 == end { skip = 0; next }
        !skip { print }
    ' "$rc_file" >"$tmp"; then
        mv "$tmp" "$rc_file" || {
            rm -f "$tmp"
            warn "cannot update $rc_file"
            return 1
        }
        say "$BRAND: removed the PATH entry from $rc_file"
    else
        rm -f "$tmp"
        warn "cannot update $rc_file"
    fi
}

if [ -z "$DESTDIR" ] && [ -n "${HOME:-}" ]; then
    for rc in "$HOME/.zshrc" "$HOME/.bashrc" "$HOME/.profile"; do
        remove_path_block "$rc"
    done
fi

cat <<EOF

$BRAND has been removed.

If '$PROG_NAME' is still found, another installation (for example
/usr/local/bin/$PROG_NAME) is still present; remove it with:

  ./scripts/uninstall.sh --system
EOF

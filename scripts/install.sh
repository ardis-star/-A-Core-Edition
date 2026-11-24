#!/bin/sh
# -A Core Edition installer.
#
# Installs the compiler as the command `-a` so it can be used from any
# directory, without entering the source tree and without shell aliases.
#
#   ./scripts/install.sh --user      install for the current user (~/.local/bin)
#   ./scripts/install.sh --system    install for every user (/usr/local/bin)
#
# Extra options (also useful for packagers and tests):
#
#   --prefix DIR    install under DIR/bin instead of the default prefix
#   --destdir DIR   stage the installation under DIR (no PATH changes)
#   --binary FILE   install FILE instead of building the compiler with cargo
#   --no-path       do not touch any shell configuration file
#   --force         replace an existing, unrelated `-a` binary without a backup
#   --quiet         print only warnings and errors
#
# The installer never overwrites an unrelated `-a`: it moves it aside to
# `<path>.bak.<timestamp>` first. It never duplicates PATH entries, and the
# installed compiler is a single self-contained executable that does not need
# the source tree to run.

set -eu

BRAND="-A Core Edition"
PROG_NAME="-a"
RC_BEGIN="# >>> -A Core Edition >>>"
RC_END="# <<< -A Core Edition <<<"

MODE="user"
PREFIX=""
DESTDIR="${DESTDIR:-}"
SETUP_PATH=1
FORCE=0
QUIET=0
BINARY="${A_BINARY:-}"

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
-A Core Edition installer

USAGE:
    ./scripts/install.sh [--user|--system] [options]

MODES:
    --user            install for the current user in ~/.local/bin (default)
    --system          install for all users in /usr/local/bin (needs root)

OPTIONS:
    --prefix DIR      install into DIR/bin instead of the default prefix
    --destdir DIR     stage files under DIR (for packaging; no PATH changes)
    --binary FILE     install this prebuilt compiler instead of building one
    --no-path         do not modify any shell configuration file
    --force           replace an existing, unrelated -a without a backup
    --quiet           only report problems
    -h, --help        show this help

After installing, run:
    -a --version
    -a create hello.ma
    -a run hello.ma
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
        --binary)
            [ $# -ge 2 ] || die "--binary needs a file"
            BINARY="$2"
            shift
            ;;
        --no-path) SETUP_PATH=0 ;;
        --force) FORCE=1 ;;
        --quiet) QUIET=1 ;;
        -h|--help) usage; exit 0 ;;
        *) usage >&2; die "unknown option '$1'" ;;
    esac
    shift
done

# ---------------------------------------------------------------- locations
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
# The directory a user's PATH must contain (without any staging prefix).
PATH_DIR="${BASE_PREFIX}/bin"

# --------------------------------------------------------------- elevation
if [ "$MODE" = "system" ] && [ -z "$DESTDIR" ] && [ "$(id -u)" != "0" ]; then
    if [ "${A_ELEVATED:-0}" = "1" ]; then
        die "still not root after elevation; run this script with sudo"
    fi
    if command -v sudo >/dev/null 2>&1; then
        say "$BRAND: system installation needs root, re-running with sudo"
        exec sudo env A_ELEVATED=1 A_BINARY="$BINARY" "$0" "$@"
    fi
    die "system installation needs root: re-run with sudo, or use --user"
fi

# ------------------------------------------------------------------ binary
if [ -z "$BINARY" ]; then
    script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
    root=$(CDPATH= cd -- "$script_dir/.." && pwd)
    [ -f "$root/Cargo.toml" ] || die "cannot find the -A sources next to $script_dir (use --binary FILE)"
    command -v cargo >/dev/null 2>&1 || die "cargo was not found; install Rust, or pass --binary FILE"
    say "$BRAND: building the release compiler (cargo build --release -p minus-a)"
    ( cd "$root" && cargo build --release -p minus-a ) || die "the build failed"
    # Honour CARGO_TARGET_DIR: a build with it set does not use $root/target.
    target_dir="${CARGO_TARGET_DIR:-$root/target}"
    case "$target_dir" in
        /*) : ;;
        *) target_dir="$root/$target_dir" ;;
    esac
    BINARY="$target_dir/release/minus-a"
fi
[ -f "$BINARY" ] || die "compiler binary '$BINARY' does not exist"
[ -x "$BINARY" ] || die "compiler binary '$BINARY' is not executable"

# ---------------------------------------------------------- collision check
is_ours() {
    candidate="$1"
    [ -f "$candidate" ] || return 1
    version=$("$candidate" --version 2>/dev/null || true)
    case "$version" in
        *"$BRAND"*) return 0 ;;
        *) return 1 ;;
    esac
}

mkdir -p "$INSTALL_DIR" || die "cannot create '$INSTALL_DIR'"

if [ -e "$TARGET" ]; then
    if is_ours "$TARGET"; then
        say "$BRAND: updating the existing installation at $TARGET"
    elif [ "$FORCE" -eq 1 ]; then
        warn "'$TARGET' exists and is not $BRAND; replacing it because --force was given"
    else
        stamp=$(date +%Y%m%d%H%M%S)
        backup="$TARGET.bak.$stamp"
        warn "'$TARGET' exists and is not $BRAND"
        mv "$TARGET" "$backup" || die "cannot move the existing '$TARGET' aside"
        warn "the existing file was backed up to $backup"
    fi
fi

# ----------------------------------------------------------------- install
if command -v install >/dev/null 2>&1; then
    install -m 0755 "$BINARY" "$TARGET" || die "cannot install to '$TARGET'"
else
    cp -f "$BINARY" "$TARGET" || die "cannot copy to '$TARGET'"
    chmod 0755 "$TARGET" || die "cannot make '$TARGET' executable"
fi
say "$BRAND: installed $TARGET"

# -------------------------------------------------------------- PATH setup
write_path_block() {
    rc_file="$1"
    dir="$2"
    if [ -f "$rc_file" ] && grep -Fq "$RC_BEGIN" "$rc_file"; then
        say "$BRAND: $rc_file already configures PATH for -A"
        return 0
    fi
    if [ -f "$rc_file" ] && grep -Fq "$dir" "$rc_file"; then
        say "$BRAND: $rc_file already mentions $dir; leaving it untouched"
        return 0
    fi
    {
        printf '\n%s\n' "$RC_BEGIN"
        printf 'export PATH="%s:$PATH"\n' "$dir"
        printf '%s\n' "$RC_END"
    } >>"$rc_file" || {
        warn "cannot write $rc_file; add $dir to PATH yourself"
        return 1
    }
    say "$BRAND: added $dir to PATH in $rc_file"
}

configure_path() {
    dir="$1"
    case ":$PATH:" in
        *":$dir:"*)
            say "$BRAND: $dir is already on PATH"
            return 0
            ;;
    esac
    shell_name=$(basename "${SHELL:-sh}")
    case "$shell_name" in
        zsh) files="$HOME/.zshrc" ;;
        bash) files="$HOME/.bashrc" ;;
        *) files="$HOME/.profile" ;;
    esac
    for rc in $files; do
        write_path_block "$rc" "$dir"
    done
    say "$BRAND: open a new terminal (or source the file above) to use -a"
}

if [ -n "$DESTDIR" ]; then
    say "$BRAND: staged installation under $DESTDIR; PATH was not modified"
elif [ "$SETUP_PATH" -eq 0 ]; then
    say "$BRAND: PATH was not modified (--no-path); ensure $PATH_DIR is on PATH"
elif [ "$MODE" = "system" ]; then
    say "$BRAND: /usr/local/bin is normally on PATH already"
else
    configure_path "$PATH_DIR"
fi

# ------------------------------------------------------------------ verify
installed_version=$("$TARGET" --version 2>/dev/null || true)
case "$installed_version" in
    *"$BRAND"*)
        say "$BRAND: verified ($installed_version)"
        ;;
    *)
        die "the installed binary did not report itself as $BRAND"
        ;;
esac

cat <<EOF

$BRAND is installed.

  command   $PROG_NAME
  location  $TARGET

Try it:

  $PROG_NAME --version
  $PROG_NAME create hello.ma
  $PROG_NAME run hello.ma

If '$PROG_NAME' is not found, open a new terminal or add this to your PATH:

  $PATH_DIR
EOF

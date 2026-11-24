# Installing -A Core Edition

After installation the compiler is the command **`-a`**, available in every
directory. It is a single self-contained executable: it does not need the source
tree, a Cargo installation, or a shell alias to run.

```console
$ -a create hello.ma
-A Core Edition: created hello.ma
$ -a run hello.ma
Hello, world!
```

## Quick install

```console
$ git clone <this repository> && cd <this repository>
$ ./scripts/install.sh            # same as --user
```

```console
$ -a --version
-A Core Edition 0.1.0
```

## The two installation modes

| Mode | Command | Location | Who can use it |
| --- | --- | --- | --- |
| user | `./scripts/install.sh --user` | `~/.local/bin/-a` | you |
| system | `./scripts/install.sh --system` | `/usr/local/bin/-a` | every user |

`--user` is the default and never needs elevated permissions. `--system` asks for
root only when you select it: the script re-runs itself through `sudo` (and says
so). If `sudo` is unavailable it stops and tells you to run it as root.

Both modes build the release compiler (`cargo build --release -p minus-a`) unless
you pass `--binary FILE`, which installs an already-built compiler — useful for
packaging and for the test suite.

## What gets installed

Exactly one file: the compiler, installed as `-a` with mode `0755`. The compiler
links the assembly backend in and invokes the system assembler/linker (`cc`) at
build time, so nothing else has to be installed alongside it.

The installed binary is independent of where the source tree lives — you can move
or delete the repository afterwards and `-a` keeps working.

## PATH configuration

`PATH` is only touched when it has to be, and never twice.

1. If the install directory is already on `PATH`, nothing is changed.
2. Otherwise the installer appends a marked block to the configuration file of
   your current shell:

   ```sh
   # >>> -A Core Edition >>>
   export PATH="/home/ardis/.local/bin:$PATH"
   # <<< -A Core Edition <<<
   ```

   * `zsh` → `~/.zshrc`
   * `bash` → `~/.bashrc`
   * anything else → `~/.profile`

3. If the block (or the directory) is already present, the file is left alone, so
   running the installer again does not duplicate entries.
4. `--no-path` skips the whole step; `--destdir` implies it.

Open a new terminal afterwards, or source the file the installer names.

## Collisions with an existing `-a`

The installer never silently replaces an unrelated program called `-a`:

```console
$ ./scripts/install.sh --user
-A Core Edition: '/home/ardis/.local/bin/-a' exists and is not -A Core Edition
-A Core Edition: the existing file was backed up to /home/ardis/.local/bin/-a.bak.20261124203314
-A Core Edition: installed /home/ardis/.local/bin/-a
```

An existing **-A** installation (recognised by its `--version` output) is simply
upgraded in place. `--force` replaces an unrelated file without keeping a backup.

## Uninstalling

```console
$ ./scripts/uninstall.sh --user      # or --system
-A Core Edition: removed /home/ardis/.local/bin/-a
-A Core Edition: removed the PATH entry from /home/ardis/.bashrc
```

The uninstaller refuses to delete a `-a` that is not -A Core Edition unless you
pass `--force`, removes the PATH block it added, and is safe to run twice.

## Options

| Option | Effect |
| --- | --- |
| `--user` / `--system` | choose the mode (default `--user`) |
| `--prefix DIR` | install into `DIR/bin` instead of the default prefix |
| `--destdir DIR` | stage the whole installation under `DIR` (packaging; no PATH changes) |
| `--binary FILE` | install a prebuilt compiler instead of building one |
| `--no-path` | never touch shell configuration |
| `--force` | replace an unrelated `-a` without a backup |
| `--quiet` | only print problems |
| `-h`, `--help` | usage |

Staged installation (what a distribution package does):

```console
$ ./scripts/install.sh --system --destdir "$PWD/debian/tmp"
-A Core Edition: installed /…/debian/tmp/usr/local/bin/-a
-A Core Edition: staged installation under /…/debian/tmp; PATH was not modified
```

## Verifying the installation

```console
$ -a --version
-A Core Edition 0.1.0
$ -a doctor
```

`doctor` reports the version, the executable's real path, whether `-a` on `PATH`
is that same compiler, which install location is populated, whether the assembler
and linker work, and whether a program can be compiled, linked and executed. It
exits non-zero if any check fails, so it can gate automation. It never modifies
the system.

## Requirements

* A C toolchain (`cc`, or `$CC`) able to assemble x86-64 Intel syntax and link
  against pthreads — needed by `build` and `run`, checked by `doctor`.
* Rust and Cargo only to *build* the compiler from source.

## A shell quirk worth knowing

Typing `-a ...` at a prompt, and using it inside a script file, behaves normally.
Passing it as the argument of `sh -c` or `bash -c` does not:

```console
$ sh -c '-a --version'          # the shell parses '-a' as its own option
sh: 0: Illegal option -
$ printf -- '-a --version\n' > run.sh && sh run.sh
-A Core Edition 0.1.0
```

This is the shell's argument parsing, not the compiler. Use a script file, or
`sh -c -- '-a --version'`, in automation.

## Development builds

Without installing anything:

```console
$ cargo build --release
$ ./target/release/minus-a run examples/hello_world.ma
```

`minus-a` and `-a` are the same program; only the installed name differs.

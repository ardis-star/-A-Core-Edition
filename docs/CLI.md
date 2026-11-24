# Command line

```
-A Core Edition — compiler for the -A language

USAGE:
    -a create  <name>                  Create a new .ma source file
    -a run     <input.ma>              Compile and run a program
    -a build   <input.ma> [-o <exe>]   Build an executable
    -a compile <input.ma> [-o <asm>]   Emit x86-64 assembly (.s)
    -a check   <input.ma>              Parse and type-check only
    -a doctor                         Diagnose the installation

OPTIONS:
    -o <path>   Output path (default: <input stem>.s for compile,
                <input stem> next to the source for build)
    --force     create: overwrite an existing file
    --no-opt    Disable the optimizer
    --            run: everything after this is passed to the program
    --help      Show this help
    --version   Show version

SOURCE FILES:
    -A sources use the .ma extension (ma is the only accepted one).

EXAMPLES:
    -a create hello.ma
    -a run hello.ma
    -a build hello.ma -o hello
    -a run ./project/main.ma

EXIT STATUS:
    0  success
    1  rejected program, failed assembly/linking/execution, or doctor problems
    2  usage error
```

The command is `-a`; the Cargo binary target is named `minus-a`. Both behave
identically — `-a` is simply how the compiler is installed (see
[INSTALL.md](INSTALL.md)).

## `-a create`

Writes a starter program that compiles and runs immediately:

```console
$ -a create hello.ma
-A Core Edition: created hello.ma
  next: -a run hello.ma
$ cat hello.ma
// hello.ma
//
// Run it with:  -a run hello.ma
// Build it to:  -a build hello.ma

block main(1) {
    aout("Hello, world!")
}
```

* `game` becomes `game.ma` — the official extension is added for you.
* Any other extension (`.a`, `.mp`, `.txt`) is refused with a usage error; the
  file is not created.
* An existing file is never overwritten: you get `E_IO` naming the file and a
  `Help:` line suggesting `--force`.

```console
$ -a create hello.ma
-A Core Edition: error
  File:    hello.ma
  Line:    1:1
  Code:    E_IO (input/output)
  Reason:  'hello.ma' already exists; nothing was written
  Help:    pass --force to overwrite it
```

## `-a run`

One command from source to running program: read, parse, type-check, resolve
imports, generate assembly, assemble, link, execute.

```console
$ -a run hello.ma
Hello, world!
```

* The program's stdout, stderr and stdin are the terminal's: output ordering and
  exit status are exactly those of the program. Parallel groups print in source
  declaration order (see [RUNTIME.md](RUNTIME.md#deterministic-parallel-output)).
* The process exit status is the program's. `fail` in a block, a runtime bounds
  error and any other non-zero exit come back unchanged:

  ```console
  $ -a run boom.ma
  error: boom
  $ echo $?
  1
  ```

* Nothing is written next to the source: the assembly and the executable live in
  a temporary directory that is removed when the program exits (even if it
  crashes).
* A rejected program never executes and leaves nothing behind.
* Imports resolve relative to the **source file**, so `-a run ./game/main.ma`
  finds `./game/math.ma` no matter which directory you invoke it from.
* Program arguments go after `--`: `-a run tool.ma -- --input data.txt`.

## `-a build`

Produces a persistent executable named after the source file:

```console
$ -a build hello.ma
-A Core Edition: wrote hello (via hello.s)
$ ./hello
Hello, world!
$ -a build hello.ma -o /usr/local/bin/hello
```

The intermediate assembly is written next to the executable it belongs to, so a
build always leaves exactly two predictable files: `<exe>` and `<exe>.s`.

## `-a compile`

Emits the generated x86-64 assembly using the established `.s` convention:

```console
$ -a compile hello.ma
-A Core Edition: wrote hello.s
$ -a compile hello.ma -o out/hello.s
```

`--no-opt` disables the optimizer, which is how the test suite compares optimized
and unoptimized behaviour.

## `-a check`

Parses and type-checks without producing any output — what a build server or an
editor wants:

```console
$ -a check hello.ma
-A Core Edition: hello.ma: OK
```

## `-a doctor`

Reports the state of the installation. It changes nothing on the system (it only
uses a temporary directory it removes again) and exits non-zero when a check
fails, so it can gate a CI job or an installation script.

```console
$ -a doctor
-A Core Edition: doctor

  [ ok ] version           -A Core Edition 0.1.0
  [ ok ] executable        /home/ardis/.local/bin/-a
  [ ok ] installed name    -a
  [ ok ] PATH              '-a' resolves to /home/ardis/.local/bin/-a
  [ ok ] user install      /home/ardis/.local/bin/-a
  [warn] system install    not installed in /usr/local/bin
  [ ok ] assembler/linker  cc — cc (Debian 14.2.0-19) 14.2.0
  [ ok ] backend           compile, assemble, link and run all work
  [ ok ] temp directory    /tmp is writable
  [ ok ] privileges        user installs need no elevation (/home/ardis/.local/bin exists)

-A Core Edition: doctor found no problems (1 warning(s))
```

What it checks:

| Check | Meaning |
| --- | --- |
| `version` | the version of the running compiler |
| `executable` | the resolved path of the binary that is running |
| `installed name` | whether it was invoked as `-a` |
| `PATH` | whether `-a` on `PATH` is *this* compiler |
| `user install` / `system install` | which of the two install locations is populated |
| `assembler/linker` | `cc` (or `$CC`) exists and which version it is |
| `backend` | a real program is compiled, assembled, linked and executed |
| `temp directory` | `-a run` can create its scratch directory |
| `privileges` | what an installation would need on this machine |

Warnings never fail the command; only `[fail]` lines do.

## Source-file rule

`.ma` is the only accepted extension, for every command:

```console
$ -a check hello_world.a
-A Core Edition: error
  File:    hello_world.a
  Line:    1:1
  Code:    E_IO (input/output)
  Reason:  'hello_world.a' is not a -A source file (found .a); -A sources use the .ma extension
  Help:    rename it to 'hello_world.ma', or create a new file with `-a create hello_world`
```

## Diagnostics

Every rejection is one structured report: file, line and column, a stable code
with a human category, the reason, and — where the compiler knows one — a
suggested correction.

```console
$ -a check fixed.ma
-A Core Edition: error
  File:    fixed.ma
  Line:    3:5
  Code:    E_SEMANTIC (name and meaning check)
  Reason:  cannot assign to 'x': it was declared with 'set' (fixed); use 'setc' for a changeable variable
  Help:    change the declaration to `setc x = ...` to allow assignments
```

| Code | Category | Example |
| --- | --- | --- |
| `E_LEX` | lexing | `unexpected character ';'` |
| `E_PARSE` | syntax | `expected '}' to close the struct literal` |
| `E_SEMANTIC` | name and meaning check | `undefined reuse 'nope' attached to block 'main'` |
| `E_TYPE` | type check | `a condition must be bool (on/off), found int` |
| `E_QUEUE` | block scheduling | `missing parallel group members in queue 2` |
| `E_IO` | input/output | `cannot read 'main.ma': No such file or directory` |
| `E_TOOLCHAIN` | assembler/linker | `could not assemble/link 'hello'` |
| `E_USAGE` | command line | `unknown command 'frobnicate'` |

Codes and the `error[...]` line are stable for scripts and editors; the library
(`minus_a::Diagnostic`) still renders the compact
`error[CODE]: message` + `--> file:line:col` form when formatted with
`Display`, and the full report with `Diagnostic::report()`.

## Library use

The same pipeline is available as a crate, including the CLI:

```rust
use minus_a::{compile_file_opts, Options};

let assembly = compile_file_opts(std::path::Path::new("main.ma"), Options { optimize: true })?;
```

`minus_a::cli::run(&args)` is the entry point the `-a` binary uses.

# -A Core Edition

The compiler for **-A**, a language of blocks, queues and reuse. Written in Rust,
it translates `.ma` source files into x86-64 assembly and assembles them into a
native executable through its own backend.

-A organises a program around *blocks*: a block is a named unit of work assigned
to an execution queue, and queues run in order with real parallel groups inside
them. Everything else in Core — values, control flow, reuse, structs, enums,
generics, traits, pointers, errors, modules, intrinsics — exists to make those
blocks do something useful.

```
reuse roll(lo, hi) { return random(lo, hi) }

block setup(1) {
    aout("rolling two dice")
}

block left(roll(1, 6), 2:2) {
    aout("left:  ", A, roll)
}

block right(roll(1, 6), 2:2) {
    aout("right: ", A, roll)
}

block report(3) {
    aout("done")
}
```

Saved as `dice.ma`, that program runs with one command (after
[installing](#install)):

```console
$ -a run dice.ma
rolling two dice
left:  4
right: 6
done
```

## Install

```console
$ cargo build --release            # or: ./scripts/install.sh --user
$ ./scripts/install.sh --user      # installs the command `-a` in ~/.local/bin
$ ./scripts/install.sh --system    # installs it in /usr/local/bin (sudo)
```

After that, from any directory:

```console
$ -a create hello.ma
$ -a run hello.ma
Hello, world!
```

`-a` is a single self-contained executable: no source tree, no Cargo, no shell
alias. See [docs/INSTALL.md](docs/INSTALL.md) for PATH handling, collisions,
staged installs, and uninstallation.

## Source files

The official extension for -A source files is **`.ma`**:

```
main.ma        hello_world.ma     random_number.ma
shapes.ma      arrays.ma         parallel_blocks.ma
```

`.ma` is the only accepted input extension. Anything else — including the old
`.a` and `.mp` spellings — is rejected before compilation starts:

```console
$ -a check main.a
-A Core Edition: error
  File:    main.a
  Line:    1:1
  Code:    E_IO (input/output)
  Reason:  'main.a' is not a -A source file (found .a); -A sources use the .ma extension
  Help:    rename it to 'main.ma', or create a new file with `-a create main`
```

## Requirements

A Rust toolchain (2021 edition) and a C toolchain (`cc`) capable of assembling
x86-64 gas/Intel syntax and linking against pthreads. See
[docs/BACKEND.md](docs/BACKEND.md) for exactly what the backend does and does
not cover.

## Command line

```console
-a create  <name>                  # write a starter .ma program
-a run     <input.ma>              # compile, assemble, link and run
-a build   <input.ma> [-o <exe>]   # produce a persistent executable
-a compile <input.ma> [-o <asm>]   # emit x86-64 assembly (.s)
-a check   <input.ma>              # parse and type-check only
-a doctor                          # diagnose the installation
```

`run` is the whole point: one command from source to running program, with the
program's own stdout, stderr and exit status. It leaves nothing behind — the
assembly and the executable live in a temporary directory.

Exit codes: `0` success, `1` the program was rejected or assembly, linking or
execution failed, `2` usage error. Diagnostics name the file, line and column,
and give a stable code, a reason and — where one exists — a suggested fix:

```console
$ -a check fixed.ma
-A Core Edition: error
  File:    fixed.ma
  Line:    3:5
  Code:    E_SEMANTIC (name and meaning check)
  Reason:  cannot assign to 'x': it was declared with 'set' (fixed); use 'setc' for a changeable variable
  Help:    change the declaration to `setc x = ...` to allow assignments
```

See [docs/CLI.md](docs/CLI.md).

## What Core Edition implements

| Area | Status |
| --- | --- |
| Blocks, queues `(N)`, parallel groups `(N:N)`, `(N:NxK)`, barriers | complete |
| Order-independent blocks, queue synchronisation | complete |
| Deterministic parallel output (declaration order, per-block buffers) | complete |
| Repeated block names (independent execution contexts) | complete |
| `aout(...)` with string literals and the `A` marker | complete |
| `set` / `setc`, `insert` (`5`, `-5`, `x5`, `/5`, array append), `remove` | complete |
| `when` / `else` / `anyelse` (first match wins) | complete |
| `loop { …, N }`, `loop { while c { … then … } }`, `each x in a { … }` | complete |
| Arrays: literals, 1-based indexing, `.length`, append, remove, mixed elements | complete |
| Reuse: attach with and without arguments, caching, `as` bindings | complete |
| Generics: monomorphized reuses and structs | complete |
| Structs (nested, fields, mutation), enums with explicit values | complete |
| Traits with compile-time signature validation, inherent `impl` | complete |
| Pointers: `&x`, `*p`, `@alloc`/`@free`, typed pointees | complete |
| Error values: `fail`, `?`, `.ok` / `.value` / `.error` | complete |
| Modules: multi-file imports, `pub` visibility, cycle detection | complete |
| Intrinsics: memory, bit ops, atomics, fences, target queries, cells | complete |
| Optimization: folding, propagation, CSE, identities, dead code | complete |
| Consolidated runtime emitted into every assembly output | complete |
| Dot-qualified hierarchy (`make.shape.circle`) | reserved, diagnosed |
| Foundation packages (graphics, math, GUI, audio) | out of scope |

## Examples

`examples/` holds programs that are compiled and executed by the test suite:

| File | Shows |
| --- | --- |
| `hello_world.ma`, `variables.ma` | values, `set`/`setc`, printing |
| `conditionals.ma`, `conditional_loop.ma`, `conditional_loop_and.ma` | `when`/`else`/`anyelse`, `and`, `then` |
| `counted_loop.ma`, `each_iteration.ma` | counted `loop`, `each` |
| `arrays.ma`, `aout_mixed.ma` | arrays, `.length`, `insert`/`remove`, mixed elements |
| `queues.ma`, `parallel_blocks.ma`, `parallel_four.ma`, `order_independent.ma` | queues, parallel groups, barriers |
| `parallel_output_order.ma` | deterministic output order inside parallel groups |
| `reuse.ma`, `reuse_parallel.ma`, `random_number.ma` | reuse attachment, caching, per-block contexts, `random` |
| `structs.ma`, `enums.ma`, `generics.ma`, `shapes.ma` | data types and polymorphism |
| `pointers.ma`, `intrinsics.ma`, `error_handling.ma` | pointers, intrinsics, results |
| `showcase.ma` | everything together |
| `modules/main.ma` (+ `math.ma`, `labels.ma`) | multi-file program |

## Tests

```console
$ cargo test --workspace
```

The suite (260 tests) compiles every program with the real compiler, assembles
the generated x86-64 assembly with the backend and executes it; output, exit
codes and stderr are asserted. Invalid programs assert on the exact diagnostic,
the CLI tests pin the `.ma` extension rule and the `-A Core Edition` branding,
differential tests run the same program optimized and unoptimized and compare
behaviour, and timing tests measure genuine parallelism. Parallel output order is
pinned exactly: a group's blocks print in source declaration order on every run,
however long each block takes. The installation tests
run `scripts/install.sh` and `scripts/uninstall.sh` against throw-away `HOME` and
`DESTDIR` directories — nothing is installed on the machine running the tests —
and then exercise the installed `-a` from an unrelated directory. Nothing is
skipped silently: if no assembler/linker is present the affected tests say so.

## Documentation

| Document | Contents |
| --- | --- |
| [LANGUAGE_SPEC.md](docs/LANGUAGE_SPEC.md) | the complete Core language |
| [TYPE_SYSTEM.md](docs/TYPE_SYSTEM.md) | types, inference, conversion rules |
| [BLOCKS_AND_QUEUES.md](docs/BLOCKS_AND_QUEUES.md) | scheduling, groups, barriers |
| [REUSE.md](docs/REUSE.md) | attached logic, caching, parameters, generics |
| [MEMORY_MODEL.md](docs/MEMORY_MODEL.md) | values, ownership, allocation |
| [RUNTIME.md](docs/RUNTIME.md) | the runtime emitted with every program |
| [ERROR_HANDLING.md](docs/ERROR_HANDLING.md) | results, `fail`, `?` |
| [MODULES.md](docs/MODULES.md) | imports, visibility, cycles |
| [INTRINSICS.md](docs/INTRINSICS.md) | `@alloc`, `@atomic_add`, `@fence`, … |
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | the compiler pipeline |
| [INSTALL.md](docs/INSTALL.md) | installing, PATH, collisions, uninstalling |
| [BACKEND.md](docs/BACKEND.md) | the assembly backend, honestly described |
| [CLI.md](docs/CLI.md) | command line, exit codes, diagnostics |

## Foundation boundary

Core Edition is the language and its compiler. It ships no `make.*` packages:
`make.shape.circle` and friends are *parsed* as dot-qualified names and rejected
with a diagnostic that names the Foundation namespace, because the libraries
that implement them are a separate product. The dot-qualified name machinery
(module paths, `math.Vec2`, `signs.Sign.positive`) is complete and used by
modules.

# Compiler architecture

One pipeline, one representation per stage, no panics on user input.

```
source files
    |  modules.rs     load imports, detect cycles, qualify names
    v
text
    |  lexer.rs      characters -> tokens
    v
tokens
    |  parser.rs     tokens -> ast::Program (recursive descent, 1 token lookahead)
    v
AST
    |  semantic.rs   names, types, queues, monomorphization, escape analysis
    v
CheckedProgram
    |  optimizer.rs  folding, propagation, CSE, identities, dead code
    v
CheckedProgram'
    |  codegen.rs    AST -> x86-64 assembly text
    v
x86-64 assembly (+ runtime)
    |  minus-a-backend        cc -x assembler -pthread -o executable
    v
native program
```

`lib.rs` exposes the whole thing:

```rust
pub fn compile_source(source: &str) -> CompileResult<String>            // optimize
pub fn compile_source_opts(src: &str, opts: Options) -> CompileResult<String>
pub fn compile_file(path: &Path) -> CompileResult<String>               // + imports
pub fn compile_file_opts(path: &Path, opts: Options) -> CompileResult<String>
pub fn check_source(source: &str) -> CompileResult<()>                  // no codegen
pub fn check_file(path: &Path) -> CompileResult<()>
```

`Options { optimize: bool }` is the only knob; `--no-opt` sets it to false.

## Stages

### `diagnostics.rs`
`Span` (1-based line and column), `Diagnostic { code, message, span, file }`,
`CompileResult<T>`. Constructors `lex_error`, `parse_error`, `semantic_error`,
`type_error`, `queue_error`, `io_error`, `toolchain_error` keep codes uniform.
`Display` renders `error[CODE]: message` plus `--> file:line:col`.

### `lexer.rs`
Tokens with spans. Keywords are matched exactly; `;` and `or` are rejected with
targeted messages rather than a generic syntax error. Strings may be single- or
double-quoted and carry escapes; newlines are significant (they terminate
statements), so the lexer preserves line breaks as token positions, and the
parser consults `at_line_break()` instead of the lexer inventing terminators.

### `parser.rs`
Recursive descent with one token of lookahead and a depth budget so pathological
input produces a diagnostic instead of a stack overflow. Statement boundaries come
from line breaks at bracket depth zero: inside `(...)` or `[...]` an expression
may span lines. `return` with no value stops at the line break (or `}`), so
`return` followed by a statement is not parsed as `return <statement>`.

### `semantic.rs`
The largest stage, and the only one that decides meaning:

* registers structs, enums, traits and reuses under canonical (module-qualified)
  names, then checks visibility, duplicates and trait-impl signatures;
* type-checks every expression with local inference, fixing each variable's type
  at its declaration;
* enforces `set`/`setc` mutability and the `insert`/`remove` rules;
* validates queue and parallel-group consistency;
* monomorphizes generic reuses and structs (instance published before the body
  is checked, so recursion terminates);
* performs the escape analysis that drives automatic release of owned locals;
* builds the execution schedule consumed by codegen.

### `optimizer.rs`
Works on the checked program and is deliberately conservative: every rewrite is
gated on purity, and a variable whose address is taken is never treated as a
constant. Passes: constant folding, incremental constant propagation (single
pass over each statement list, so a literal bound earlier is visible to later
statements), copy propagation, common-subexpression elimination, algebraic
identities (`x+0`, `x*1`, `x*0`, …), and dead-code elimination. The optimizer is
validated by differential tests: identical programs are compiled with and without
it and the outputs compared.

### `codegen.rs`
Emits Intel-syntax x86-64 assembly: `.intel_syntax noprefix`, a stack frame per reuse
and per block (16-byte aligned), values in `rax`/`xmm0` with a uniform 16-byte
tagged slot for aggregates, calls to the runtime for anything non-trivial, and
generated `main` that initialises the runtime, runs the queues in order with
`pthread_create`/`pthread_join` for parallel groups, shuts the runtime down and
returns 0.

### `cli/`
The user-facing surface, one module per concern: `cli/mod.rs` (argument parsing,
dispatch, the `.ma` rule, `check` and `compile`), `cli/create.rs` (the starter
template and the extension rules), `cli/toolchain.rs` (assemble, link, execute;
the self-cleaning scratch directory used by `run`) and `cli/doctor.rs` (the
read-only environment report). `main.rs` is a two-line shell around
`minus_a::cli::run`.

### `runtime.rs`
The runtime itself, as x86-64 assembly source (see [RUNTIME.md](RUNTIME.md)), emitted
verbatim into every output under unreferenced-by-default `__a_*` symbols (only
`main` is `.globl`).

### `backend` (`minus-a-backend`)
The assembly backend: writes the assembly file, invokes `cc -x assembler
-pthread`, and surfaces assembler/linker failures as `E_TOOLCHAIN`
diagnostics. See [BACKEND.md](BACKEND.md).

## Testing strategy

* `tests/support/mod.rs` — compile, assemble, link, run, and capture
  stdout/stderr/exit status; helpers `run_program`, `run_program_opts`,
  `run_program_timed`, `run_files`, `check_files`, `compile_ok`, `compile_err`.
* `tests/compiler_tests.rs` — end-to-end programs and optimizer differentials.
* `tests/core_features.rs` — the feature surface: types, control flow, reuse,
  generics, traits, pointers, intrinsics, modules, runtime misuse, examples.
* `tests/invalid_programs.rs` — negative tests: every diagnostic, with the
  message text asserted.
* `tests/cli_tests.rs` — the command line: `--help`/`--version`, `create` (extension
  rule, `--force`, refusal to overwrite), `compile`/`build`/`check`, `run`
  (output, exit status, runtime errors, imports, no leftover files), `doctor`,
  and usage/exit-code behaviour.
* `tests/install_tests.rs` — `scripts/install.sh` and `scripts/uninstall.sh`
  against throw-away `HOME`/`DESTDIR` directories: user and staged system
  installs, PATH de-duplication, collision backup, uninstall round trip, and the
  installed `-a` running a program from an unrelated directory.

Tests that need a toolchain detect its absence and say so on stderr rather than
pretending to pass; timing tests measure execution only (never build time) and
retry until they get a clean measurement.

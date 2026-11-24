# The -A Core Edition assembly backend

-A never emits machine code or object files itself. The compiler's output is
**x86-64 assembly source text**; this document describes exactly what that means,
what the backend does, and where its limits are.

## What the backend is

```
main.ma  --(minus-a)-->  x86-64 assembly (.s)  --(minus-a-backend)-->  executable
```

* `minus-a` — the compiler. Lexes, parses, type-checks, optimizes and generates
  assembly.
* `minus-a-backend` — the assembly backend: a driver that validates the generated
  assembly file, invokes the system assembler and linker, and reports failures.

The backend is an internal component of -A Core Edition. It is not a separate
language, and nothing in this repository claims compatibility with an assembler
implementation that does not ship here. What it targets is precisely *x86-64 GNU
assembler, Intel syntax, System V AMD64 ABI*, assembled and linked with

```console
$ cc -x assembler -pthread <input.s> -o <executable>
```

The backend never rewrites its input and never reports success when the
assembler or linker failed: a failure surfaces as an `E_TOOLCHAIN` diagnostic.

`CC` selects a different assembler/linker if required.

## Required structure of a generated assembly file

```asm
# comments start with '#'
.intel_syntax noprefix      # Intel operand order throughout

.text
.globl main                 # the entry symbol is `main`, not `_start`
.type main, @function
main:
    ...
```

Rules the generated code obeys:

1. **Entry point is `main`.** Defining `_start` collides with the C runtime
   startup objects the linker pulls in.
2. **`.intel_syntax noprefix`** at the top: `mov rax, 1`, not `movq $1, %rax`.
3. **`.note.GNU-stack`** is declared, so the binary does not request an
   executable stack.
4. Generated symbols are prefixed (`__a_*` for the runtime and block/reuse
   bodies, `.L*` for local labels). Only `main` is `.globl`; everything else is
   file-local, which is why the runtime cannot collide with libc.
5. Data lives in `.rodata` (strings, double constants), `.data` (enum objects,
   which hold pointers) and `.bss` (globals, runtime state, the print lock), with
   explicit `.balign` where alignment matters.

## ABI facts the generated code relies on

* Integer and pointer arguments: `rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`.
* Return value: `rax`; `rdx` for the high half of a division, which is where `%`
  takes its remainder from.
* Floating-point arguments: `xmm0`…; `al` holds the number of vector registers
  used when calling variadic functions such as `printf`.
* `rsp` must be 16-byte aligned *at* each `call`. On entry `rsp % 16 == 8`, so
  after `push rbp` it is 0 and every frame is a multiple of 16.
* `rbp` is the frame pointer; locals live at negative offsets from it.
* Callee-saved registers: `rbx`, `rbp`, `r12`–`r15`. Generated code touches only
  caller-saved registers (`rax`, `rcx`, `rdx`, `rsi`, `rdi`, `r8`–`r11`), except
  where it saves and restores what it uses.

## External symbols used

| Symbol | Purpose |
| --- | --- |
| `printf`, `sprintf`, `fprintf`, `exit` | output and fatal errors |
| `malloc`, `free` | allocation |
| `memcpy`, `memmove`, `memset`, `strlen`, `strcmp` | strings and element buffers |
| `pthread_create`, `pthread_join` | parallel groups and queue barriers |
| `pthread_mutex_init`, `pthread_mutex_lock`, `pthread_mutex_unlock` | `aout` line atomicity |

Arithmetic that must be atomic across threads uses hardware atomics rather than
library locks; the random state, for instance, is advanced with

```asm
lock xadd QWORD PTR [rip + __a_random_state], rax
```

## Verified capabilities

Each of these was assembled and executed before the compiler relied on it:

* `main` entry; `printf` with `%lld`, `%s`, `%.15g`, `0x%llx`;
* `.data`/`.bss` globals and `.rodata` literals;
* `lock xadd` on a global, `lock cmpxchg` for compare-and-swap;
* real `pthread_create`/`pthread_join` with two workers running concurrently;
* mutex-protected output from multiple threads;
* a single translation unit with one global symbol (`main`), `.note.GNU-stack`,
  and PIE linking without text relocations (enum objects live in `.data`, not
  `.rodata`).

Anything not listed is not assumed to work. If a future target gains new
instructions or a different object format, `codegen.rs`, `runtime.rs` and the
`backend` crate are the only places that need to change: the front end and the
language are independent of the backend.

## Portability

The target is x86-64 Linux. On other targets the front end is unchanged, but
`codegen.rs`, `runtime.rs` and `backend` would need a matching implementation.
The intrinsics `@arch()`, `@os()` and `@pointer_bits()` report the target rather
than pretending it is fixed.

## What the backend is not

* It is not an assembler of its own: it delegates to `cc`, and says so in
  `--help`.
* It is not a second language: there is no "-A assembly" syntax to learn beyond
  the assembly listed above, and no user-facing name for it other than "the
  assembly backend".
* It is not optional magic: if `cc` is missing, `-a build` and `-a run` fail with
  `E_TOOLCHAIN` and say so, while `-a compile` and `-a check` keep working.

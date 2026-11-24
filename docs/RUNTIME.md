# The -A Core Edition runtime

Every program the compiler emits carries one runtime, written in x86-64 assembly and
living in `minus-a/src/runtime.rs`. Code generation never inlines runtime
semantics: it *calls* these entry points, which is why there is exactly one place
to look when output, allocation or error behaviour needs to change.

The runtime is emitted into every output under the `__a_` prefix. Nothing in it
is exported: `main` is the only global symbol, so the runtime cannot collide with
user symbols or with libc.

## Entry points

| Helper | Arguments (System V) | Result |
| --- | --- | --- |
| `__a_init` | — | — |
| `__a_runtime_shutdown` | — | — |
| `__a_alloc` | `rdi` = payload bytes | `rax` = payload |
| `__a_release` | `rdi` = payload | — |
| `__a_live_allocs` | — | `rax` = count |
| `__a_array_create` | — | `rax` |
| `__a_array_append` | `rdi` array, `rsi` tag, `rdx` value | — |
| `__a_array_get` | `rdi` array, `rsi` 1-based index | `rax` = cell |
| `__a_array_set` | `rdi`, `rsi`, `rdx` tag, `rcx` value | — |
| `__a_array_remove` | `rdi` array, `rsi` index | — |
| `__a_array_free` | `rdi` array | — |
| `__a_struct_new` | `rdi` = field count | `rax` |
| `__a_struct_set` | `rdi`, `rcx` index, `rsi` tag, `rdx` value | — |
| `__a_result_ok` | `rdi` tag, `rsi` value | `rax` |
| `__a_result_err` | `rdi` message | `rax` |
| `__a_print_int/double/string/bool/ptr` | value | — |
| `__a_print_array/struct/result` | pointer | — |
| `__a_random` | `rdi` low, `rsi` high | `rax` |
| `__a_cell_new` / `__a_cell_get` | value / cell | cell / value |
| `__a_print_lock_acquire/release` | — | — |
| `__a_out_raw` | `rdi` bytes, `rsi` length | — |
| `__a_buf_new` | — | `rax` = buffer |
| `__a_buf_append` | `rdi` buffer, `rsi` bytes, `rdx` length | — |
| `__a_buf_flush` | `rdi` buffer (may be `0`) | — |
| `__a_bit_*`, `__a_atomic_*`, `__a_fence` | see [INTRINSICS.md](INTRINSICS.md) | — |
| `__a_fail_raw` / `__a_fail_exit` | `rdi` = message | does not return |
| `__a_error_index/div_zero/type/random/oom` | — | does not return |

## Initialisation and shutdown

`__a_init` runs before the first block: it initialises the print mutex and seeds
the random generator. `__a_runtime_shutdown` runs after the last queue: it frees
every allocation still registered, so a leaked object is reclaimed at exit rather
than blamed on the program.

## Output

Every printer funnels into `__a_out_raw(bytes, length)`, which is the only place
that writes to stdout. It chooses its destination from `r15`:

* `r15 == 0` (the main thread, and therefore every sequential block and all of
  the program's own top-level statements): the bytes go straight to `stdout`, so
  interactive output appears as soon as it is produced.
* `r15 != 0` (a member of a parallel group): the bytes are appended to that
  member's own buffer. The buffer object is three words — `[length][capacity]`
  and a pointer to the data block — so growth (`realloc`) never moves the object
  the thread holds in `r15`, only the data behind it. The data block starts at
  256 bytes and doubles as needed.

Because of that, a parallel group's output can never interleave: see
[Deterministic parallel output](#deterministic-parallel-output).

`__a_print_string`, `_bool` and `_newline` pass their bytes through directly;
`__a_print_int`, `_double` and `_ptr` format with `snprintf` into a 64-byte stack
buffer first (the value is at most a few dozen characters) and then hand the
result to `__a_out_raw`, clamped to what fits. The composite printers
(`_cell`, `_array`, `_struct`, `_result`) are unchanged: they are built from the
primitive ones, so they inherit the routing for free.

`aout` still compiles to `__a_print_lock_acquire`, the print calls and
`__a_print_lock_release`. The lock is a `pthread_mutex_t` in `.bss` (a plain
zeroed mutex is not valid, so `__a_init` calls `pthread_mutex_init`); it keeps
writes from different threads from tearing each other apart and is cheap enough
to leave in place now that group members write to private buffers.

`r15` is the one register the runtime reserves. Generated code never touches it,
and it is callee-saved, so it survives calls into reuse bodies and into nested
blocks. Every runtime helper that only uses `rdi`/`rsi`/`rdx`/`rax`/`xmm*`
preserves it, which is what lets a single register select the output sink for a
whole thread.

## Deterministic parallel output

A parallel group runs its blocks at the same time, and their output is collected
per block, then flushed in source declaration order once the whole group has been
joined. The generated `main` does exactly this:

1. allocate one buffer per member (`__a_buf_new`) and keep the pointer in the
   frame;
2. `pthread_create` the member's worker trampoline (`__a_block_N_worker`),
   passing that buffer as the thread argument — the trampoline moves it into
   `r15` and then calls the block body;
3. `pthread_join` every member;
4. walk the members in declaration order, calling `__a_buf_flush` on each
   buffer, which writes it with `fwrite`, `fflush`es `stdout` and frees it;
5. only then continue with the next queue.

The order comes from the schedule, never from completion time, so a group prints
the same text on every run no matter which block is fastest. The order of
`aout()` calls inside a block is preserved for the same reason: they all append
to one buffer in program order.

When a block dies (`fail`, bounds error, division by zero, …) the failing thread
first flushes its own buffer and then writes the message to stderr, so output
produced before the failure is not lost. Output already buffered by the *other*
members of the group is not printed in that case: the process exits before the
group is joined.

Because a group's text is held in memory until the group ends, a block that
prints an enormous amount inside a parallel group uses memory in proportion to
what it prints. Sequential blocks are unaffected: they write straight through.

The fatal-error paths flush the calling thread's output buffer, write with
`fprintf(stderr, …)` and never touch the print lock: they must not deadlock if the
faulting thread already holds it. They then `exit(1)` without unwinding.

## Fatal errors

`fail` in a block and the runtime checks (bounds, division by zero, type
mismatch, out-of-memory, bad random range) all print a message to stderr and exit
with status 1 through the same helper.

## Static data

String literals, double constants and enum objects are emitted into `.rodata`,
except enum objects, which hold a pointer to their variant name and therefore
live in `.data`. The compiled program is a PIE, so this avoids text relocations.

## Why assembly and not C

-A compiles to assembly, so the runtime is emitted as assembly rather than linked
in as a C object. The only external symbols are libc
(`malloc`, `realloc`, `free`, `snprintf`, `fwrite`, `fflush`, `strlen`, `memset`)
and pthread
(`pthread_create`, `pthread_join`, `pthread_mutex_*`) — the documented external
surface of a compiled -A program.

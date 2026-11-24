# Intrinsics

Intrinsics are written `@name(args)` and are the only way to reach operations
the language itself cannot express: allocation, raw memory, atomics, bit
operations, fences and target queries. They are **not** inline assembly — each
one maps to a documented runtime routine, so the generated assembly stays
readable and portable, and the whole set can be implemented by any backend for
-A Core.

| Intrinsic | Signature | Result | Notes |
| --- | --- | --- | --- |
| `@alloc(size)` | int bytes | `ptr` | zeroed payload; out-of-memory is fatal |
| `@free(p)` | pointer | — | unlinks the allocation; null is ignored |
| `@live_allocs()` | — | int | allocations currently registered |
| `@cell(value)` | any | cell | heap box for a value |
| `@unbox(cell)` | cell | value | reads the boxed value |
| `@mem_read8(p)` | pointer | int | unsigned byte load |
| `@mem_read64(p)` | pointer | int | 64-bit load |
| `@mem_write8(p, v)` | pointer, int | — | byte store |
| `@mem_write64(p, v)` | pointer, int | — | 64-bit store |
| `@mem_copy(dst, src, len)` | pointers, int | — | `memcpy` semantics |
| `@mem_set(dst, byte, len)` | pointer, int, int | — | `memset` semantics |
| `@popcount(x)` | int | int | number of set bits |
| `@clz(x)` | int | int | leading zeros (64-bit) |
| `@ctz(x)` | int | int | trailing zeros |
| `@bswap(x)` | int | int | byte-swap the 64-bit word |
| `@rotl(x, n)` / `@rotr(x, n)` | int, int | int | rotate left/right |
| `@atomic_add(p, v)` | pointer, int | int | returns the **previous** value |
| `@atomic_sub(p, v)` | pointer, int | int | previous value |
| `@atomic_swap(p, v)` | pointer, int | int | previous value |
| `@atomic_cas(p, old, new)` | pointer, int, int | bool | true when the swap happened |
| `@fence()` | — | — | full memory barrier |
| `@arch()` | — | string | `"x86_64"` |
| `@os()` | — | string | `"linux"` |
| `@pointer_bits()` | — | int | `64` |
| `@has_atomics()` | — | bool | latency-free target query |

`random(min, max)` is a plain builtin (not an intrinsic): it returns an integer in
`[min, max]` and aborts with `error: random range invalid (min > max)` if the
range is inverted.

## Atomics in practice

```
set counter = @alloc(8)
@mem_write64(counter, 0)

block left(1:1) {
    loop { @atomic_add(counter, 1), 20000 }
}
block right(1:1) {
    loop { @atomic_add(counter, 1), 20000 }
}
block report(2) {
    aout(A, @mem_read64(counter))   // exactly 40000
}
```

The test suite runs exactly this shape and asserts `40000` every time: parallel
threads really race, and the atomics really serialise them.

## Argument checking

Intrinsics are type-checked like functions:

```
aout(A, @popcount("x"))
// error[E_TYPE]: @popcount expects an int argument, found string
```

Unknown intrinsics are reported by name (`unknown intrinsic '@frobnicate'`), and
`@` without a name is a parse error.

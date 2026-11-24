# Memory model

## Uniform representation

Every value in -A Core is two machine words wide at most, so a variable is a
16-byte slot: a runtime *tag* and the value. That single decision makes the rest
of the model simple:

* a variable lives in its block's (or reuse's) stack frame;
* an array element is such a slot, so arrays can be mixed;
* a struct is a heap object holding a count and an array of such slots;
* copying a variable copies two words; no deep copy, no reference counting for
  values you never mutate.

Strings are C strings. Array elements that hold a string hold the pointer.

## Heap objects and the allocation header

Every heap object comes from `__a_alloc`, which prepends a 16-byte header and
links it into a program-wide registry:

```
header (16 bytes, returned pointer - 16):
    [+0]  next-link  (intrusive list, head in __a_alloc_head)
    [+8]  size       (payload bytes as requested)
    [+16] payload …  (the pointer the program sees)
```

`@free` unlinks and releases. `@live_allocs()` returns the number of live
objects, which makes leak behaviour testable:

```
set buf = @alloc(32)
aout(A, @live_allocs())   // 1
@free(buf)
aout(A, @live_allocs())   // 0
```

At exit, `__a_runtime_shutdown` frees everything still registered, so a program
that never calls `@free` still leaves the operating system nothing behind.

## Automatic release of owned locals

Values created inside a reuse or block that nobody else can observe are released
when the frame ends:

* arrays returned to the caller are *not* released (the caller owns them);
* arrays that stay local are released through `__a_array_free`, which frees the
  element buffer and then the object header;
* other owned locals are released through `__a_release`.

That is why `scratch_size()` in `examples/pointers.ma` can build a 4-element array
and the program still reports `0` live allocations afterwards.

## Pointers

```
setc x = 41
set p = &x       // address of a variable
*p = 42          // write through it
aout(A, *p)      // 42
```

`&x` marks the variable as *address-taken*, which matters for the optimizer: a
variable whose address escapes is never replaced by a constant, so `*p = 42`
cannot be silently undone by a stale copy of `41`.

`@alloc`/`@free` give explicit control, and `@mem_write*`/`@mem_read*` let you
access raw memory:

```
set buf = @alloc(32)
@mem_write64(buf, 1234)
aout(A, @mem_read64(buf))
@free(buf)
```

## Cells

`@cell` boxes a value on the heap and `@unbox` reads it back; cells are the
foundation of the synchronisation primitives:

```
set c = @cell(41)
aout(A, @unbox(c) + 1)
```

## Arrays

Arrays are dynamic: `insert a = x` appends, growing capacity geometrically;
`remove a[i]` shifts the tail down. `insert a = 5` adds five to the *first*
element, `insert a = x5` multiplies it, `/5` divides it, `-5` subtracts five.
Appending to a mixed array stores the element with the tag of its actual type,
and writing an element of a different type is a runtime error.

Indexing is 1-based; the bounds check prints `error: array index out of bounds`
and exits.

## Threads

Each parallel block runs on its own thread with its own stack, so frames never
alias. Shared state is expressed with `@alloc` + `@atomic_add`/`@atomic_swap`/
`@atomic_cas` and `@fence`, and the runtime's print lock keeps output lines
whole.

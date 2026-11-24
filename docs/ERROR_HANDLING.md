# Error handling

-A Core has two kinds of failure, and they are deliberately different.

## 1. Compile-time errors

Everything the compiler can decide statically is a compile error with a code and
a location:

```
error[E_TYPE]: cannot assign string to 'x' of type int (types are fixed at declaration)
  --> main.ma:4:5
```

Nothing is generated when a program has a diagnostic. See [CLI.md](CLI.md) for
the codes.

## 2. Failure values

A runtime condition that is part of the program's logic is a *value*. A reuse
that executes `fail` produces a result instead of a plain value:

```
reuse divide(a, b) {
    when b == 0 { fail "division by zero" }
    return a / b
}
```

Callers receive an object with three fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `.ok` | bool | did the reuse succeed |
| `.value` | T | the returned value (only when `.ok`) |
| `.error` | string | the message (only when not `.ok`) |

```
set r = divide(10, 2)
when r.ok { aout("result: ", A, r.value) }
anyelse { aout("error: ", A, r.error) }
```

Printing a result directly gives `ok <value>` or `failed: <error>`:

```
aout(A, divide(10, 0))   // failed: division by zero
```

A reuse that never executes `fail` is not fallible: it returns a bare value and
calling `.ok` on it is a type error. That is decided per reuse, so you do not pay
for error plumbing you do not use.

## Propagation with `?`

`expr?` returns early from the enclosing reuse, passing the failure on:

```
reuse inner(x) {
    when x < 0 { fail "negative input" }
    return x * 2
}
reuse outer(x) {
    set v = inner(x)?      // on failure, outer fails with the same message
    return v + 1
}
```

Repeated propagation keeps the original message: `outer(-1)` yields `failed:
negative input`, not a wrapped or re-worded error.

## `fail` in a block

A block has no caller to return to, so `fail` there is fatal: the message goes to
stderr as `error: <message>` and the program exits with status 1.

```
block main(1) {
    when @live_allocs() > 100 { fail "allocation leak" }
    aout("fine")
}
```

## Runtime faults

Conditions the compiler cannot check are caught by the runtime and reported the
same way, printing `error: …` to stderr and exiting 1:

| Situation | Message |
| --- | --- |
| `a[0]`, `a[n+1]` | `error: array index out of bounds` |
| `1 / 0`, `1.0 / 0.0` | `error: division by zero` |
| mixed-array element of the wrong type | `error: array element type mismatch` |
| `random(6, 1)` | `error: random range invalid (min > max)` |
| allocation failure | `error: out of memory` |

## Guidelines

* Return a result when the caller can do something about the failure; `fail`
  when it is a bug or an unrecoverable state.
* `?` is for plumbing, not for hiding: the message arrives unchanged, so write
  messages a user can act on.
* Nothing is thrown, nothing unwinds, no global error state exists: a failure is
  a value you can store, compare, print and return.

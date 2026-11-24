# Reuse: -A's reusable logic

`reuse` is the -A mechanism for naming a piece of logic and giving its result a
name inside a block. It is not a function call in the C sense: attaching a reuse
to a block *evaluates it once, at the start of that block's execution*, and binds
the result.

## Plain attachment

```
reuse answer() { return 42 }

block main(answer, 1) {
    aout(A, answer)
}
```

The block header lists the reuse by name; the body refers to the result by the
same name. The result is stored once, so every reference in the body sees the
same value:

```
reuse ticket() { return random(1, 1000000) }
block show(ticket, 1) {
    aout(A, ticket)   // identical on all three lines
    aout(A, ticket)
    aout(A, ticket)
}
```

## Attachment with arguments (parameterized reuse)

A reuse may declare parameters and be attached with arguments:

```
reuse double_it(x) { return x * 2 }
block main(double_it(21), 1) { aout(A, double_it) }

reuse roll(lo, hi) { return random(lo, hi) }
block dice(roll(1, 6), 1) { aout(A, roll) }
```

Arguments are ordinary expressions, evaluated in the block's context before the
body runs. Several reuses may be attached at once; the final item in the header
is always the queue:

```
block main(roll(1, 6), double_it(21), 1) {
    aout("roll: ", A, roll)
    aout("twice: ", A, double_it)
}
```

## Local bindings with `as`

When the reuse name is not the name you want in the body, bind it:

```
block main(add(1, 2) as total, add(10, 20), 1) {
    aout(A, total)   // 3
    aout(A, add)     // 30
}
```

## Reuse as an expression

A reuse can also be called wherever an expression is allowed, which is how
reuses compose:

```
reuse inc(x) { return x + 1 }
reuse twice(x) { return inc(inc(x)) }
block main(1) { aout(A, twice(40)) }
```

## Evaluation contexts

Each *block execution* is one context. Two blocks in a parallel group therefore
evaluate their attached reuses independently, and a reuse attached to a block in
queue 2 is evaluated when that queue runs, not before.

## Generics

```
reuse identity<T>(value: T) -> T { return value }

aout(A, identity(7))       // int instance
aout(A, identity("text"))  // string instamce
```

Every distinct argument-type combination is monomorphized into its own machine
code body (`__a_reuse_identity__int`, …). A generic type may also be inferred
from a struct literal (`Box { value = 42 }`).

## Recursion

```
reuse fact(n) {
    when n <= 1 { return 1 }
    return n * fact(n - 1)
}
```

Recursive and mutually recursive reuses are supported: the instance is published
before its body is checked, so the compiler terminates on `fact(n - 1)` while the
generated code calls itself normally.

## Fallibility

A reuse that executes `fail` returns a result; `?` propagates a failure:

```
reuse inner(x) {
    when x < 0 { fail "negative" }
    return x
}
reuse outer(x) {
    set v = inner(x)?
    return v * 2
}
```

A reuse that never fails returns its value directly. See
[ERROR_HANDLING.md](ERROR_HANDLING.md).

## Trait methods and inherent methods

A method is a reuse whose first parameter is `self`:

```
trait Shape { reuse area(self) -> double }
impl Shape for Circle {
    reuse area(self) -> double { return 3.14159 * self.radius * self.radius }
}
impl Circle {
    reuse radius_squared(self) -> double { return self.radius * self.radius }
}
```

`self` is the receiver, so `c.area()` resolves to the impl for `Circle`'s type.

## Rules and diagnostics

* Attaching a name that is not a reuse: `undefined reuse 'nope' attached to block
  'main'`.
* Attaching a reuse that returns nothing: `reuse 'nothing' returns nothing, so it
  cannot be attached to a block`.
* Wrong argument count: `f expects 2 argument(s) but 1 were given`.
* Duplicate definition in one module: `duplicate reuse 'f'`.
* A private reuse used from another module: `reuse 'math.secret' is private to
  module 'math'; mark it 'pub' to use it elsewhere`.

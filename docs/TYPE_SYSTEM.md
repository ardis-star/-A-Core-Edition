# The -A Core type system

## Types

| Type | Written as | Runtime value |
| --- | --- | --- |
| integer | `int` | 64-bit signed in `rax` |
| double | `double` | IEEE-754 binary64 in `xmm0` |
| boolean | `bool` | `0`/`1` in `rax`, written `on`/`off` |
| string | `string` | pointer to NUL-terminated bytes |
| array | `[T]` | pointer to `[length][capacity][data]` |
| struct | `Point`, `Box<T>` | pointer to `[count][pad][{tag,value}…]` |
| enum | `Color` | pointer to static `[value][name]` |
| result | `result<T>` | pointer to `[ok][tag][value][error]` |
| pointer | `ptr<T>` | raw address |

Arrays hold 16-byte cells: a runtime tag plus the value. That is what makes
mixed arrays (`[10, "hello", on, 19.99]`) work: each element remembers what it
is, and printing dispatches on the tag.

## Where types come from

Types are inferred locally and statically:

* literals have the obvious type (`1` is `int`, `1.0` is `double`);
* `set`/`setc` fix the type of the variable at the declaration and never widen
  it afterwards;
* a reuse parameter without an annotation takes the type of the argument at each
  instantiation, and an annotated parameter keeps its annotation (`reuse
  scale(x: int)`, `reuse area(self) -> double`);
* a generic parameter `T` is bound per instantiation and substituted into the
  body, the parameters and the return type.

There is no implicit conversion except `int` → `double` where a double is
required. Assigning a string to an int variable is a compile error, not a
runtime surprise:

```
setc x = 1
x = "s"        // error[E_TYPE]: cannot assign string to 'x' of type int
```

## Comparisons and equality

* `==` / `!=` work for `int`, `double`, `bool`, `string` (by content), enums (by
  identity) and pointers.
* `<`, `>`, `<=`, `>=` work for numbers and strings.
* Aggregates (structs, arrays, results) have no equality: comparing them would
  compare addresses, so it is rejected. Compare the fields you care about.

## Conditions

A `when`, `else` or `while` condition must be `bool`. `when 5 { … }` is a type
error; write `when x != 0 { … }`. Combine conditions with `and`; `or` does not
exist in Core.

## Generic instantiation

Each distinct set of argument types produces a separate instance:

```
reuse identity<T>(value: T) -> T { return value }

aout(A, identity(7))       // __a_reuse_identity_int
aout(A, identity("text"))  // __a_reuse_identity_string
```

Instantiation is recursive and memoized: an instance is published before its
body is checked, so `fact(n - 1)` inside `fact` resolves to the instance being
built instead of recursing forever in the compiler. Two instantiations with the
same argument types share one body.

## Trait signatures

A trait declares the exact shape of its methods. An `impl` must match:

```
trait Shape { reuse area(self) -> double }
impl Shape for Circle {
    reuse area(self) -> double { return 3.14159 * self.radius * self.radius }
}
```

Mismatched parameter counts or return types, or a missing method, are compile
errors (`E_TYPE` / `E_SEMANTIC`). `self` binds to the implementing type, so its
type is not compared literally.

## Type errors you will actually see

| Code | Example message |
| --- | --- |
| `E_TYPE` | `arithmetic '+' requires numeric operands, found int and string` |
| `E_TYPE` | `a condition must be bool (on/off), found int` |
| `E_TYPE` | `cannot assign string to 'x' of type int (types are fixed at declaration)` |
| `E_TYPE` | `cannot compare P values with '==': only numbers, strings, booleans, enums and pointers support equality` |
| `E_SEMANTIC` | `struct P has no field 'y'` |
| `E_SEMANTIC` | `enum Color has no variant 'blue'` |

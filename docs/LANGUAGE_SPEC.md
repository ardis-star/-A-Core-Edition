# -A Core Edition — Language Specification

This document describes the language implemented by this compiler. Everything
here is executable: each construct is covered by tests that compile the program
and run it.

## 1. Program structure

-A source files use the `.ma` extension: `main.ma`, `hello_world.ma`,
`shapes.ma`. A program is one or more such files, each a sequence of
declarations followed by optional *top-level statements*:

```
program     := import* decl* stmt*
import      := "import" path
path        := IDENT ("." IDENT)*
decl        := "pub"? (block | reuse | struct | enum | trait | impl)
```

An `import` names another `.ma` file without its extension (`import math` loads
`math.ma` next to the importing file); see [MODULES.md](MODULES.md).

A program must contain at least one block. Top-level statements run once at
startup, before any block; `set`/`setc` at top level declares a **program-wide**
variable that every block can read.

```
setc counter = 0          // program-wide, visible in every block

block main(1) {
    insert counter = 1
    aout(A, counter)
}
```

## 2. Lexical structure

* Identifiers: `[A-Za-z_][A-Za-z0-9_]*`. Variables, blocks and reuses are
  `snake_case`; types are `CamelCase`. Wrong case is a compile error.
* Keywords: `block reuse struct enum trait impl for pub import module fail
  return set setc when else anyelse loop while then and each insert remove aout
  on off self A`. `A` and `on`/`off` are reserved words; `A` is the evaluation
  marker used inside `aout(...)`.
* `or` is **not** part of Core: it is rejected with a diagnostic that suggests
  nested `when`/`else` conditions. Combine conditions with `and`.
* Integers: decimal, `i64`. Doubles: `1.5`, `2.0`. Booleans: `on`, `off`.
* Strings: single or double quotes, interchangeable, with `\n`, `\t`, `\\`, `\"`,
  `\'` escapes. A string literal cannot span lines.
* Comments: `// line` and `/* block */`.
* There are no semicolons and no statement terminators.

### Statement boundaries

A statement ends at the line break, so

```
set p = &x
*p = 42
```

is two statements. Inside parentheses or brackets an expression may continue
across lines, which is how long argument lists and array literals are formatted:

```
aout("total: ", A, total,
     " items: ", A, count)
```

## 3. Blocks, queues and scheduling

```
block   := "block" NAME "(" attaches? queue ")" "{" stmt* "}"
attaches := (IDENT ("(" expr,* ")")? ("as" IDENT)? ",")*
queue   := INT (":" INT ("x" INT)?)?
```

* `(N)` — the block runs alone in queue `N`.
* `(N:N)` — two blocks form a parallel group in queue `N`.
* `(N:NxK)` — `K` blocks form a parallel group in queue `N` (`K >= 2`).

Queues run in ascending order; all blocks of a queue finish before the next
queue starts (a barrier). Blocks in a parallel group run on real OS threads, and
the group's output is printed in source declaration order: each block prints into
its own buffer, and the buffers are flushed in declaration order after every
block in the group has finished.
Two blocks may not share a bare `(N)` queue, and the group size must match the
number of blocks. Block names may repeat: every scheduled declaration is an
independent execution context with its own locals.

See [BLOCKS_AND_QUEUES.md](BLOCKS_AND_QUEUES.md).

## 4. Statements

| Statement | Meaning |
| --- | --- |
| `set name = expr` | bind a fixed value (cannot be reassigned) |
| `setc name = expr` | bind a changeable value |
| `name = expr` | assign to a changeable variable |
| `target = expr` | assign to a field `p.x`, element `a[i]` or pointee `*p` |
| `insert name = value` | add (`5`), subtract (`-5`), multiply (`x5`), divide (`/5`) or append to an array |
| `remove a[i]` | remove one element (indices shift down) |
| `aout(...)` | print; see below |
| `when c { }` `else c { }` `anyelse { }` | conditional chain |
| `loop { … }` | counts executions of the body |
| `loop { while c { … then … } }` | conditional loop with a once-only terminating section |
| `each x in a { … }` | iterate an array in order |
| `return expr?` | leave a reuse (only valid inside a reuse) |
| `fail expr` | produce a failure value |

`aout(...)` takes string literals and `A` markers; every `A` marker must be
followed by a comma and an expression, and the expression is evaluated and
printed:

```
aout("value: ", A, x, " done")
```

Printing rules: ints as decimal, doubles with `%.15g`, booleans as `on`/`off`,
strings as text, arrays as `[a, b, c]` using each element's runtime tag, structs as
`P {x = 1, y = 2}`, enums as `Color.green`, results as `ok <value>` or
`failed: <error>`, pointers as `0x...`.

Conditional chains evaluate top to bottom and **only the first matching branch
runs**; `anyelse` runs when nothing matched.

`loop { … }` repeats the body and requires a count:

```
loop {
    aout("tick"),
    3
}
```

## 5. Expressions

```
expr    := or_expr
or_expr := and_expr ("?" )*
and_expr:= cmp ("and" cmp)*
cmp     := add (("<"|">"|"<="|">="|"=="|"!=") add)?
add     := mul (("+"|"-") mul)*
mul     := unary (("*"|"/"|"%") unary)*
unary   := ("-"|"&"|"*") unary | postfix
postfix := primary ("[" expr "]" | "." IDENT ("(" args ")")? | "(" args ")")*
primary := INT | DOUBLE | STRING | "on" | "off" | IDENT | "(" expr ")"
         | "[" expr,* "]" | NAME "{" field = expr,* "}" | "@" IDENT "(" args ")"
```

Notes:

* Indexing is **1-based**. `a[0]` and `a[n+1]` are runtime errors.
* `.length` works on arrays and strings.
* `.ok`, `.value`, `.error` read a result.
* Struct fields use `.field`; enum variants use `Enum.variant` and
  `Enum.variant.value`.
* `&x` takes an address, `*p` reads through a pointer.
* `expr?` propagates a failure to the caller of the enclosing reuse.

## 6. Types

`int`, `double`, `string`, `bool`, `T[]` (array), `ptr<T>`, struct types, enum
types, `result<T>`. See [TYPE_SYSTEM.md](TYPE_SYSTEM.md).

## 7. Reuse

`reuse` is -A's name for reusable logic. It is declared once and *attached* to a
block, which evaluates it once and binds the result:

```
reuse answer() { return 42 }
block main(answer, 1) { aout(A, answer) }
```

Reuse also supports parameters, `as` bindings, generics, trait methods and
recursion, and a call inside an expression. See [REUSE.md](REUSE.md).

## 8. Structs, enums, generics, traits

```
struct Point { x: int, y: int }
enum Color { red, green, blue }
enum Status { ok = 200, not_found = 404 }

struct Box<T> { value: T }
reuse identity<T>(value: T) -> T { return value }

trait Shape { reuse area(self) -> double }
impl Shape for Circle { reuse area(self) -> double { return 3.14159 * self.radius * self.radius } }
impl Circle { reuse radius_squared(self) -> double { return self.radius * self.radius } }
```

Struct literals separate fields with commas or line breaks. Every field must be
given, no unknown fields are allowed. Struct and array values have no `==`:
compare fields instead. Enums compare by identity, print as `Enum.variant`, and
expose their numeric value through `.value`.

Generics are monomorphized: `identity(7)` and `identity("x")` produce two
distinct machine-code bodies. Trait impls are validated against the trait's
declared signatures at compile time.

## 9. Error values

A reuse that executes `fail` (directly or through `?`) returns a result instead
of a bare value:

```
reuse divide(a, b) {
    when b == 0 { fail "division by zero" }
    return a / b
}

set r = divide(10, 0)
when r.ok { aout("ok: ", A, r.value) }
anyelse { aout("error: ", A, r.error) }
```

`fail` inside a block prints `error: <message>` to stderr and exits with status
1. See [ERROR_HANDLING.md](ERROR_HANDLING.md).

## 10. Modules

```
// math.ma
module math
pub struct Vec2 { x: int, y: int }
pub reuse add(a, b) { return a + b }

// main.ma
import math
block main(1) { aout(A, math.add(1, 2)) }
```

Imports resolve relative to the importing file. A `module` header must match the
path the file is imported through. Only `pub` declarations are visible outside
their module. Import cycles, missing files and duplicate declarations are
compile errors. See [MODULES.md](MODULES.md).

## 11. Intrinsics

`@name(args)` form the operations the language cannot express: allocation, raw
memory, atomics, bit manipulation, fences and target queries. They are not
inline assembly — each maps to a documented runtime routine. See
[INTRINSICS.md](INTRINSICS.md).

## 12. Runtime failures

These abort the program with a message on stderr and a non-zero exit code:

| Message | Cause |
| --- | --- |
| `error: array index out of bounds` | 1-based index outside `1..=length` |
| `error: division by zero` | integer or double division by zero |
| `error: array element type mismatch` | array element written with another type |
| `error: random range invalid (min > max)` | `random(hi, lo)` |
| `error: out of memory` | allocation failure |
| `error: <message>` | `fail` in a block |

## 13. Reserved and absent

* `or` — use `and` with nested `when`.
* `make.*` — the -A Foundation namespace. Dot-qualified names parse, and a use
  of `make.*` is rejected with a diagnostic naming Foundation.
* No inline assembly, no `goto`, no exceptions, no classes/objects, no operator
  overloading, no string concatenation operator, no implicit numeric-to-string
  conversion (print with `aout`).

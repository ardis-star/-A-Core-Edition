# Modules

A program may span several files. `import` names a file; the file declares which
module it is; the importer reaches its declarations through the module name.

## Example

`math.ma`:

```
module math

pub struct Vec2 { x: int, y: int }

pub enum Sign { positive, negative }

pub reuse add(a, b) { return a + b }

pub reuse sign_of(x) {
    when x < 0 { return Sign.negative }
    return Sign.positive
}

reuse internal_secret() { return 0 }   // not pub: private to this module
```

`main.ma`:

```
import math

block main(1) {
    aout(A, math.add(20, 22))
    set v = math.Vec2 { x = 3, y = 4 }
    aout(A, math.add(v.x, v.y))
}
```

Run it from the directory containing both files:

```console
$ -a run main.ma
$ -a build main.ma -o prog && ./prog
```

The whole example lives in `examples/modules/` and is built by the test suite.

## Rules

* `import path` — the path is a dotted name; `import math` loads `math.ma`,
  `import util.io` loads `util/io.ma`. A path is resolved relative to the file
  that contains the import, so nested imports work from a subdirectory.
* The imported file's first declaration must be `module <path>`, and it must
  match the path it was imported through:
  `error[E_SEMANTIC]: module 'math' is imported as 'math2'; a module header must
  match the import path`.
* Declarations are namespaced: `math.add`, `math.Vec2`, `math.Sign.positive`.
  Inside the module itself the short names work.
* `pub` makes a declaration visible outside its module.
  `struct`, `enum`, `trait` and `reuse` can all be `pub`. A non-`pub`
  declaration used from another module is an error:
  `reuse 'math.internal_secret' is private to module 'math'; mark it 'pub' to use
  it elsewhere`.
* The same file imported twice is loaded once.
* Import cycles are detected and reported instead of looping:
  `error[E_SEMANTIC]: cyclic import detected: main.ma -> a -> b -> a. -A Core
  does not support import cycles`.
* Two declarations with the same canonical name are a duplicate-definition
  error; two modules may each declare their own `Shape` without conflict.

## Naming

The compiler tracks a *canonical* name for every declaration: the module path
joined with the local name. `math.Vec2` and `Vec2` inside `math` are the same
type, which is why a struct passed from one module to another keeps its identity
and its methods.

Enum variants keep their qualified spelling when printed (`math.Sign.positive`),
so output identifies the module a value came from.

## What modules do not do

* No re-export, no aliasing, no wildcard import, no conditional import.
* No per-symbol access control beyond `pub` (everything private is private to the
  file).
* No separate compilation or object files: importing is textual inclusion with
  namespacing, resolved before type checking, and the whole program is compiled
  and linked as one unit.

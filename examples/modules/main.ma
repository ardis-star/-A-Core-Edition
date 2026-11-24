// Modules: several files, one program.
//
// `import math` loads `math.ma` from this file's directory. Its declarations are
// reached through the module name (`math.add`, `math.Vec2`), and only `pub`
// declarations are visible from outside. `import` inside an imported module
// pulls in further files, and a cycle is a compile error rather than a hang.

import math
import labels

block main(1) {
    aout("add:        ", A, math.add(20, 22))
    aout("sign of -7: ", A, math.sign_of(-7))
    aout("sign of  7: ", A, math.sign_of(7))

    set origin = math.Vec2 { x = 0, y = 0 }
    set corner = math.Vec2 { x = 3, y = 4 }
    aout("length:     ", A, math.length(origin, corner))

    aout(A, labels.prefix(), " via labels")
}

// Traits: shared behaviour, validated at compile time.
//
// A trait declares the signature; every impl must provide it. Calling a method
// resolves to the implementation of the receiver's type, so `area()` means the
// circle version for a Circle and the square version for a Square. Types can
// also carry methods directly with `impl Type { ... }`.

trait Shape {
    reuse area(self) -> double
    reuse name(self) -> string
}

struct Circle {
    radius: double
}

struct Square {
    side: double
}

impl Shape for Circle {
    reuse area(self) -> double {
        return 3.14159 * self.radius * self.radius
    }
    reuse name(self) -> string {
        return "circle"
    }
}

impl Shape for Square {
    reuse area(self) -> double {
        return self.side * self.side
    }
    reuse name(self) -> string {
        return "square"
    }
}

impl Square {
    reuse perimeter(self) -> double {
        return 4.0 * self.side
    }
}

block main(1) {
    set c = Circle { radius = 2.0 }
    set s = Square { side = 3.0 }

    aout(A, c.name(), " area ", A, c.area())
    aout(A, s.name(), " area ", A, s.area())
    aout("square perimeter ", A, s.perimeter())
}

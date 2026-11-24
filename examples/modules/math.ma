module math

// A struct exported to importers.
pub struct Vec2 {
    x: int
    y: int
}

// Variants of an exported enum print with their module-qualified name.
pub enum Sign {
    positive
    negative
}

pub reuse add(a, b) {
    return a + b
}

pub reuse sign_of(x) {
    when x < 0 {
        return Sign.negative
    }
    return Sign.positive
}

pub reuse length(a: Vec2, b: Vec2) -> int {
    set dx = b.x - a.x
    set dy = b.y - a.y
    return dx * dx + dy * dy
}

// Not marked `pub`: this one stays private to the module, and importing code
// that calls it fails to compile.
reuse internal_secret() {
    return 0
}

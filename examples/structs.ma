// Structs: named records with typed fields.
//
// `set` binds a value that cannot change; `setc` binds one that can, and its
// fields may then be written. Fields keep their declared type, so assigning a
// string to an int field is a compile error rather than a runtime surprise.

struct Point {
    x: int
    y: int
}

struct Segment {
    from: Point
    to: Point
    label: string
}

block main(1) {
    set origin = Point { x = 0, y = 0 }
    setc corner = Point { x = 3, y = 4 }

    aout("origin: ", A, origin.x, ",", A, origin.y)
    aout("corner: ", A, corner.x, ",", A, corner.y)

    corner.x = 10
    aout("moved corner: ", A, corner.x, ",", A, corner.y)

    set edge = Segment {
        from = origin
        to = corner
        label = "edge"
    }
    aout("segment ", A, edge.label, ": ", A, edge.from.x, " -> ", A, edge.to.x)
}

// Enums: a fixed set of named variants with numeric values.
//
// A variant prints with its enum name, compares by identity, and exposes its
// number through `.value` for things like protocol codes.

enum Status {
    ok = 200
    not_found = 404
    server_error = 500
}

enum Direction {
    north
    east
    south
    west
}

reuse describe(code) {
    when code == Status.ok.value { return "OK" }
    when code == Status.not_found.value { return "Not Found" }
    return "Server Error"
}

block main(1) {
    set good = Status.ok
    set missing = Status.not_found

    aout("good: ", A, good)
    aout("missing: ", A, missing)

    when good == Status.ok {
        aout("compared equal to Status.ok")
    }
    anyelse {
        aout("unexpected")
    }

    aout("code ", A, missing.value, " is ", A, describe(missing.value))
    aout("code ", A, Status.ok.value, " is ", A, describe(Status.ok.value))

    set heading = Direction.east
    aout("heading: ", A, heading)
}

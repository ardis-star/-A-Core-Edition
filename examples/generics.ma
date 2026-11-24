// Generics: reusable logic and containers specialized per type.
//
// `identity<T>(value: T) -> T` is instantiated once per argument type, so the
// int and string versions are separate machine-code bodies that share no state.

reuse identity<T>(value: T) -> T {
    return value
}

reuse pick<T>(first: T, second: T) -> T {
    return first
}

struct Box<T> {
    value: T
}

struct Pair<T, U> {
    first: T
    second: U
}

block main(1) {
    aout("int:    ", A, identity(42))
    aout("string: ", A, identity("text"))
    aout("double: ", A, identity(2.5))
    aout("bool:   ", A, identity(on))

    aout("picked: ", A, pick(7, 9))
    aout("picked: ", A, pick("left", "right"))

    set number = Box { value = 10 }
    set word = Box { value = "hello" }
    aout("boxed:  ", A, number.value)
    aout("boxed:  ", A, word.value)

    set both = Pair { first = 1, second = "one" }
    aout("pair:   ", A, both.first, " / ", A, both.second)
}

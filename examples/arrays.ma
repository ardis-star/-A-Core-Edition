// Arrays are 1-based, may mix types, expose .length, and support
// insert / remove on mutable (setc) arrays.
block arrays(1) {
    set names = ["ardis", "alex", "jordan"]
    setc stack = [1, 2, 3]

    aout("names[1] = ", A, names[1])
    aout("names[2] = ", A, names[2])
    aout("names[3] = ", A, names[3])
    aout("names.length = ", A, names.length)

    insert stack = 4
    aout("after insert: ", A, stack[4], " length ", A, stack.length)

    remove stack[2]
    aout("after remove: ", A, stack[2], " length ", A, stack.length)
}

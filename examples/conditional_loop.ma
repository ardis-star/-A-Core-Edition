// A conditional loop. The section before `then` repeats while the
// condition holds; the section after `then` runs exactly once, when the
// condition becomes false.
block count_up(1) {
    setc number = 0

    loop {
        while number < 3 {
            insert number = 1
            aout("running ", A, number)
            then
            aout("finished")
        }
    }
}

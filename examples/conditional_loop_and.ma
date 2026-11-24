// Multiple conditions are joined with `and`. `or` is not part of Core.
block bounded(1) {
    setc number = 0

    loop {
        while number < 5 and number > -1 {
            insert number = 1
            then
            aout("stopped at ", A, number)
        }
    }
}

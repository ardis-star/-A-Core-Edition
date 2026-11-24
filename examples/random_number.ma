// Random numbers: `random(min, max)` and how blocks draw their own values.
//
// `random` is inclusive on both ends and needs min <= max. Each block execution
// draws independently, which is what makes attached reuse useful: two parallel
// blocks that attach `roll` get two different dice.
//
// A reuse keeps one value for the whole block; calling `random` inline draws a
// new number at each call. The self-check below only relies on the documented
// range, so it never fails.

reuse roll() { return random(1, 6) }

block check(1) {
    set low = 10
    set high = 20

    loop {
        set value = random(low, high)
        when value < low {
            fail "random() returned a value below the minimum"
        }
        when value > high {
            fail "random() returned a value above the maximum"
        },
        5
    }
    aout("5 draws from 10..20 all landed in range")
}

block dice(roll, 2:2) {
    aout("left die attaches roll once: ", A, roll, " ", A, roll)
}

block dice(roll, 2:2) {
    aout("right die attaches roll once: ", A, roll, " ", A, roll)
}

block inline(3) {
    aout("inline draws: ", A, random(1, 6), " ", A, random(1, 6))
}

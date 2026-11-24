// A reuse is reusable logic that is not scheduled on its own.
// Attaching it to a block (block name(reuse, queue)) evaluates it once,
// at block start, and caches the result for that execution.
reuse random() {
    return random(1, 100)
}

block check_number(random, 1) {
    aout("cached value: ", A, random)
    aout("same value again: ", A, random)

    when random > 50 {
        aout("High number!")
    }
    anyelse {
        aout("Low number!")
    }
}

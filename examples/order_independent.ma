// Blocks are declared 3, 1, 2 but execute 1 -> 2 -> 3.
// Source position never decides execution order.
block third_step(3) {
    aout("  3. third")
}

block first_step(1) {
    aout("  1. first")
}

block second_step(2) {
    aout("  2. second")
}

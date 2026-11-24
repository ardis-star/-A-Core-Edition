// Three blocks run at the same time, but their output is always printed in
// source declaration order: hello, then meow, then third.
// The first block is deliberately the slowest, and the order still holds.
block main(1:1x3) {
    setc spin = 0
    loop {
        insert spin = 1,
        400000
    }
    aout("hello world")
}

block main(1:1x3) {
    aout("meow")
}

block main(1:1x3) {
    aout("third")
}

// The same guarantee applies inside larger groups and after a barrier.
block left(2:2) {
    aout("left")
}

block right(2:2) {
    setc spin = 0
    loop {
        insert spin = 1,
        200000
    }
    aout("right")
}

block finish(3) {
    aout("queue 3 runs after the group has been flushed")
}

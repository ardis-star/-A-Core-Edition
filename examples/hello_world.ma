// The smallest -A program: a single scheduled block in queue 1.
// No main() is needed; blocks run automatically in queue order.
block hello(1) {
    aout("Hello world!")
}

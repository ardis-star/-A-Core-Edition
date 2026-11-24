// Two parallel blocks attach the same reuse but each gets its own
// independent execution and its own cached result.
reuse random() {
    return random(1, 1000)
}

block text_label(random, 1:1) {
    aout("label saw ", A, random)
}

block text_button(random, 1:1) {
    aout("button saw ", A, random)
}

block done(2) {
    aout("done")
}

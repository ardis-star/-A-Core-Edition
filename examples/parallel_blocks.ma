// A parallel group: both blocks are in queue 1 and run at the same time.
// Queue 2 waits for both of them to finish.
block text_label(1:1) {
    setc i = 0
    loop {
        while i < 3 {
            aout("label work ", A, i)
            insert i = 1
            then
        }
    }
}

block text_button(1:1) {
    setc i = 0
    loop {
        while i < 3 {
            aout("button work ", A, i)
            insert i = 1
            then
        }
    }
}

block render(2) {
    aout("both parallel blocks finished -> render")
}

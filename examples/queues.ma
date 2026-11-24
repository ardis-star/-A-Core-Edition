// Each queue is a stage. Queue N+1 starts only after every block in
// queue N has finished.
block prepare_data(1) {
    setc rows = 0
    loop {
        while rows < 3 {
            insert rows = 1
            then
        }
    }
    aout("prepared rows: ", A, rows)
}

block process_data(2) {
    aout("processing...")
}

block display_result(3) {
    aout("done")
}

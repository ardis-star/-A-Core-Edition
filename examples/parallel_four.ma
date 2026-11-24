// Four blocks in one parallel group use the x4 form. 1:1:1:1 is not valid.
block first_task(1:1x4) {
    aout("task 1")
}

block second_task(1:1x4) {
    aout("task 2")
}

block third_task(1:1x4) {
    aout("task 3")
}

block fourth_task(1:1x4) {
    aout("task 4")
}

block finish(2) {
    aout("all four finished")
}

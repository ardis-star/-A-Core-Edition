// aout() mixes literal text and evaluated values. The A marker means
// "evaluate the expression that follows and print its value".
block main(1) {
    set name = "Ardis"
    set values = [10, "hello", on, 19.99]

    aout("Hello world!")
    aout("Name: ", A, name)
    aout("Mixed array: ", A, values[1], " / ", A, values[2], " / ",
         A, values[3], " / ", A, values[4])
    aout("Length: ", A, values.length)
}

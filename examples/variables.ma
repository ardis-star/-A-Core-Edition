// set is a fixed value, setc is a changeable value. Types are inferred
// at declaration and never change afterwards.
block variables(1) {
    set age = 16
    set price = 19.99
    set name = "Ardis"
    set active = on
    setc score = 100

    score = 150
    score = score + 15

    aout("age: ", A, age)
    aout("price: ", A, price)
    aout("name: ", A, name)
    aout("active: ", A, active)
    aout("score: ", A, score)
}

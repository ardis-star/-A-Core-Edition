// showcase.ma - exercises the whole -A Core surface in one program.

reuse dice() {
    return random(1, 6)
}

block setup(1) {
    setc total = 0
    setc names = ["ardis", "alex"]

    insert names = "jordan"

    each name in names {
        aout("registered: ", A, name)
    }

    aout("names length: ", A, names.length)
}

block roll(dice, 2:2) {
    aout("first die: ", A, dice)
    aout("first die again (cached): ", A, dice)
}

block roll_again(dice, 2:2) {
    aout("second die: ", A, dice)
}

block report(3) {
    setc score = 100
    score = 150
    score = score + 15
    aout("score: ", A, score)

    when score > 200 {
        aout("legendary")
    }
    else score > 160 {
        aout("great")
    }
    anyelse {
        aout("good")
    }

    loop {
        aout("counted line"),
        2
    }

    setc n = 0
    loop {
        while n < 2 {
            insert n = 1
            then
            aout("loop finished at ", A, n)
        }
    }
}

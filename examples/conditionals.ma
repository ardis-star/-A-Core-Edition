// when / else / anyelse: evaluated top to bottom, first match wins.
block grade_check(1) {
    setc score = 15

    when score > 90 {
        aout("Excellent")
    }
    else score > 50 {
        aout("Pass")
    }
    else score > 10 {
        aout("Barely")
    }
    anyelse {
        aout("Failed")
    }
}

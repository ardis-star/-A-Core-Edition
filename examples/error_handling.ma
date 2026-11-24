// Error handling: values, not exceptions.
//
// A reuse that can fail returns a result. `fail` produces the error case with a
// message; `?` propagates it to the caller unchanged. Nothing is thrown and no
// global error state is involved, so a failure is an ordinary value you can
// print, compare and return.

reuse parse_positive(x) {
    when x < 0 { fail "negative input" }
    when x == 0 { fail "zero input" }
    return x * 2
}

reuse validated(x) {
    // `?` re-raises the failure of parse_positive.
    set value = parse_positive(x)?
    return value + 1
}

reuse report(result) {
    when result.ok {
        return "ok"
    }
    return "failed"
}

block main(1) {
    set good = validated(10)
    when good.ok {
        aout("value: ", A, good.value)
    }
    anyelse {
        aout("error: ", A, good.error)
    }

    set bad = validated(-3)
    when bad.ok {
        aout("unexpected success")
    }
    anyelse {
        aout("error: ", A, bad.error)
    }

    set zero = validated(0)
    aout("zero input -> ", A, zero)

    aout("good result is ", A, report(good))
    aout("bad result is ", A, report(bad))
}

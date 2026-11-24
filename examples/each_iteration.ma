// each walks an array sequentially; no index variable is required.
block each_iteration(1) {
    set names = ["ardis", "alex", "jordan"]
    setc total = 0

    each name in names {
        aout("name: ", A, name)
    }

    each n in [1, 2, 3, 4] {
        insert total = n
    }

    aout("total: ", A, total)
}

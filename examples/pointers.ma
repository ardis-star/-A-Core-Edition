// Pointers and the hybrid memory model.
//
// `&x` takes the address of a variable and `*p` reads or writes through it.
// Heap memory comes from `@alloc` and is returned with `@free`; anything still
// live when the program ends is released by the runtime shutdown. Values you
// allocate in a reuse and never free are released automatically when the reuse
// returns, which the `@live_allocs()` counts show.

reuse scratch_size() {
    // This array is local to the reuse: it is released on return.
    set buffer = [0, 0, 0, 0]
    return buffer.length
}

block main(1) {
    setc counter = 41
    set p = &counter

    aout("counter before: ", A, counter)
    *p = counter + 1
    aout("counter after:  ", A, counter)
    aout("through pointer: ", A, *p)

    set heap = @alloc(32)
    @mem_write64(heap, 1234)
    aout("heap value: ", A, @mem_read64(heap))
    aout("live allocations: ", A, @live_allocs())

    @free(heap)
    aout("after free: ", A, @live_allocs())

    aout("scratch length: ", A, scratch_size())
    aout("live allocations: ", A, @live_allocs())
}

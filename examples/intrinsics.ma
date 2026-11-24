// Intrinsics: the operations only the backend can provide.
//
// Intrinsics are written `@name(args)`. They are not inline assembly: each one
// maps to a documented runtime/backend operation, so the generated -A assembly stays
// portable and readable.

block main(1) {
    aout("target: ", A, @arch(), " / ", A, @os())
    aout("pointer bits: ", A, @pointer_bits())
    when @has_atomics() {
        aout("atomics: available")
    }

    aout("popcount(255) = ", A, @popcount(255))
    aout("clz(1) = ", A, @clz(1))
    aout("ctz(8) = ", A, @ctz(8))
    aout("rotl(1, 4) = ", A, @rotl(1, 4))
    aout("rotr(16, 4) = ", A, @rotr(16, 4))
    aout("bswap(1) = ", A, @bswap(1))

    set counter = @alloc(8)
    @mem_write64(counter, 40)

    aout("atomic add returns the old value: ", A, @atomic_add(counter, 2))
    aout("counter now: ", A, @mem_read64(counter))

    when @atomic_cas(counter, 42, 100) {
        aout("compare-and-swap succeeded")
    }
    aout("counter now: ", A, @mem_read64(counter))

    @fence()
    @free(counter)
}

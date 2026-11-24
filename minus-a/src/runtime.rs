//! The -A Core runtime, written in x86-64 assembly.
//!
//! This is the single place where runtime behaviour lives: allocation and
//! release, the allocation registry, output, arrays, structs, results, strings,
//! random numbers, bit helpers, atomics helpers and fatal error reporting.
//! Code generation only ever calls these entry points, so runtime semantics are
//! not scattered through the backend.
//!
//! Calling conventions (System V AMD64):
//!
//! | helper                     | arguments                        | result |
//! |----------------------------|----------------------------------|--------|
//! | `__a_alloc`                | `rdi` = payload size             | `rax`  |
//! | `__a_release`              | `rdi` = payload pointer          | -      |
//! | `__a_live_allocs`          | -                                | `rax`  |
//! | `__a_array_create`         | -                                | `rax`  |
//! | `__a_array_append`         | `rdi` arr, `rsi` tag, `rdx` value| -      |
//! | `__a_array_get`            | `rdi` arr, `rsi` 1-based index   | `rax` cell |
//! | `__a_array_set`            | `rdi`,`rsi`,`rdx` tag,`rcx` value| -      |
//! | `__a_array_remove`         | `rdi` arr, `rsi` 1-based index   | -      |
//! | `__a_struct_new`           | `rdi` = field count              | `rax`  |
//! | `__a_struct_set`           | `rdi`,`rcx` index,`rsi` tag,`rdx`| -      |
//! | `__a_result_ok`            | `rdi` tag, `rsi` value           | `rax`  |
//! | `__a_result_err`           | `rdi` = error string             | `rax`  |
//! | `__a_out_raw`              | `rdi` bytes, `rsi` length        | -      |
//! | `__a_buf_new`              | -                                | `rax`  |
//! | `__a_buf_flush`            | `rdi` buffer                     | -      |
//! | `__a_print_*`              | see names                        | -      |
//!
//! Output ordering: a parallel-group member runs with `r15` pointing at its own
//! output buffer, so every `aout()` appends to that member's buffer instead of
//! touching stdout. The parent thread flushes the buffers in source declaration
//! order once the whole group has been joined. Sequential blocks run with
//! `r15 == 0` and write straight through, which keeps interactive output live.
//! `r15` is the only register the runtime reserves; generated code never uses
//! it, and it is callee-saved, so it survives calls into reuse bodies.
//!
//! Memory model: every allocation carries a 16-byte header linking it into the
//! runtime registry. `@free` (and automatic release of non-escaping locals)
//! removes the node; `__a_runtime_shutdown` releases whatever remains when the
//! program ends. `@live_allocs()` exposes the registry size, which is how the
//! tests verify that automatic release really happens.

/// Architecture reported by `@arch()`.
pub const ARCH: &str = "x86_64";
/// OS reported by `@os()`.
pub const OS: &str = "linux";

pub const HELPERS: &str = r#"
.text

# ---------------------------------------------------------------- runtime init
__a_init:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    lea rdi, [rip + __a_print_lock]
    xor rsi, rsi
    call pthread_mutex_init
    movabs rax, 0x853c49e6748fea9b
    mov QWORD PTR [rip + __a_random_state], rax
    leave
    ret

# ------------------------------------------------------------- allocator core
# __a_alloc(size) -> payload pointer
# Layout: [next][size][payload...]
__a_alloc:
    push rbp
    mov rbp, rsp
    sub rsp, 32
    mov QWORD PTR [rbp - 8], rdi
    lea rdi, [rdi + 16]
    call malloc
    test rax, rax
    je __a_error_oom
    mov QWORD PTR [rbp - 16], rax
    mov rcx, QWORD PTR [rip + __a_alloc_head]
    mov QWORD PTR [rax], rcx
    mov rcx, QWORD PTR [rbp - 8]
    mov QWORD PTR [rax + 8], rcx
    mov QWORD PTR [rip + __a_alloc_head], rax
    inc QWORD PTR [rip + __a_alloc_count]
    add rax, 16
    leave
    ret

# __a_release(pointer) -- unlink and free; ignores unknown or null pointers
__a_release:
    push rbp
    mov rbp, rsp
    sub rsp, 48
    test rdi, rdi
    je .Lrel_done
    mov QWORD PTR [rbp - 8], rdi
    mov rax, QWORD PTR [rip + __a_alloc_head]
    xor rcx, rcx
.Lrel_loop:
    test rax, rax
    je .Lrel_done
    lea rdx, [rax + 16]
    cmp rdx, QWORD PTR [rbp - 8]
    je .Lrel_found
    mov rcx, rax
    mov rax, QWORD PTR [rax]
    jmp .Lrel_loop
.Lrel_found:
    mov rdx, QWORD PTR [rax]
    test rcx, rcx
    je .Lrel_head
    mov QWORD PTR [rcx], rdx
    jmp .Lrel_free
.Lrel_head:
    mov QWORD PTR [rip + __a_alloc_head], rdx
.Lrel_free:
    dec QWORD PTR [rip + __a_alloc_count]
    mov rdi, rax
    call free
.Lrel_done:
    leave
    ret

# __a_array_free(array) -- release the element buffer, then the header.
__a_array_free:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    test rdi, rdi
    je .Laf_done
    mov QWORD PTR [rbp - 8], rdi
    mov rdi, QWORD PTR [rdi + 16]
    test rdi, rdi
    je .Laf_header
    call __a_release
.Laf_header:
    mov rdi, QWORD PTR [rbp - 8]
    call __a_release
.Laf_done:
    leave
    ret

__a_live_allocs:
    mov rax, QWORD PTR [rip + __a_alloc_count]
    ret

# Release everything still registered (called once, at program exit).
__a_runtime_shutdown:
    push rbp
    mov rbp, rsp
    sub rsp, 16
.Lsh_loop:
    mov rax, QWORD PTR [rip + __a_alloc_head]
    test rax, rax
    je .Lsh_done
    mov rcx, QWORD PTR [rax]
    mov QWORD PTR [rip + __a_alloc_head], rcx
    dec QWORD PTR [rip + __a_alloc_count]
    mov rdi, rax
    call free
    jmp .Lsh_loop
.Lsh_done:
    leave
    ret

# -------------------------------------------------------------------- output
#
# All output goes through __a_out_raw, which either appends to the current
# thread's buffer (r15 != 0, i.e. a parallel-group member) or writes straight to
# stdout (r15 == 0). Formatting happens with snprintf into a small stack buffer,
# so no printer needs to know which mode it is in.

__a_print_lock_acquire:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    lea rdi, [rip + __a_print_lock]
    call pthread_mutex_lock
    leave
    ret

__a_print_lock_release:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    lea rdi, [rip + __a_print_lock]
    call pthread_mutex_unlock
    leave
    ret

# __a_out_raw(bytes, length) -- append to the current buffer or write to stdout
__a_out_raw:
    push rbp
    mov rbp, rsp
    sub rsp, 32
    test rsi, rsi
    je .Lraw_done
    mov QWORD PTR [rbp - 8], rdi
    mov QWORD PTR [rbp - 16], rsi
    test r15, r15
    je .Lraw_direct
    mov rdi, r15
    mov rsi, QWORD PTR [rbp - 8]
    mov rdx, QWORD PTR [rbp - 16]
    call __a_buf_append
    jmp .Lraw_done
.Lraw_direct:
    mov rdi, QWORD PTR [rbp - 8]
    mov esi, 1
    mov rdx, QWORD PTR [rbp - 16]
    mov rcx, QWORD PTR [rip + stdout]
    call fwrite
.Lraw_done:
    leave
    ret

# __a_buf_new() -> buffer object
#
# A buffer object is three words -- [length][capacity][data] -- and is itself
# stable: growth replaces the data block, never the object, because the owning
# thread keeps the object address in r15 for its whole life.
__a_buf_new:
    push rbp
    mov rbp, rsp
    sub rsp, 32
    mov edi, 24
    call malloc
    test rax, rax
    je __a_error_oom
    mov QWORD PTR [rbp - 8], rax
    mov edi, 256
    call malloc
    test rax, rax
    je __a_error_oom
    mov rcx, QWORD PTR [rbp - 8]
    mov QWORD PTR [rcx], 0
    mov QWORD PTR [rcx + 8], 256
    mov QWORD PTR [rcx + 16], rax
    mov rax, rcx
    leave
    ret

# __a_buf_append(buffer, bytes, length)
__a_buf_append:
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov QWORD PTR [rbp - 8], rdi
    mov QWORD PTR [rbp - 16], rsi
    mov QWORD PTR [rbp - 24], rdx
    test rdx, rdx
    je .Lap_done
    mov rax, QWORD PTR [rbp - 8]
    mov rcx, QWORD PTR [rax]
    add rcx, QWORD PTR [rbp - 24]
    mov QWORD PTR [rbp - 32], rcx
    cmp rcx, QWORD PTR [rax + 8]
    jbe .Lap_copy
    mov rdx, QWORD PTR [rax + 8]
    add rdx, rdx
    cmp rdx, QWORD PTR [rbp - 32]
    jae .Lap_grow
    mov rdx, QWORD PTR [rbp - 32]
.Lap_grow:
    mov QWORD PTR [rbp - 40], rdx
    mov rdi, QWORD PTR [rax + 16]
    mov rsi, rdx
    call realloc
    test rax, rax
    je __a_error_oom
    mov rcx, QWORD PTR [rbp - 8]
    mov QWORD PTR [rcx + 16], rax
    mov rdx, QWORD PTR [rbp - 40]
    mov QWORD PTR [rcx + 8], rdx
.Lap_copy:
    mov rax, QWORD PTR [rbp - 8]
    mov rdi, QWORD PTR [rax + 16]
    add rdi, QWORD PTR [rax]
    mov rsi, QWORD PTR [rbp - 16]
    mov rdx, QWORD PTR [rbp - 24]
    call memcpy
    mov rax, QWORD PTR [rbp - 8]
    mov rcx, QWORD PTR [rbp - 32]
    mov QWORD PTR [rax], rcx
.Lap_done:
    leave
    ret

# __a_buf_flush(buffer) -- write the buffered output, then release the buffer
__a_buf_flush:
    push rbp
    mov rbp, rsp
    sub rsp, 32
    test rdi, rdi
    je .Lfl_done
    mov QWORD PTR [rbp - 8], rdi
    mov rax, rdi
    mov rdx, QWORD PTR [rax]
    test rdx, rdx
    je .Lfl_release
    mov rdi, QWORD PTR [rax + 16]
    mov esi, 1
    mov rcx, QWORD PTR [rip + stdout]
    call fwrite
.Lfl_release:
    mov rdi, QWORD PTR [rip + stdout]
    call fflush
    mov rax, QWORD PTR [rbp - 8]
    mov rdi, QWORD PTR [rax + 16]
    call free
    mov rdi, QWORD PTR [rbp - 8]
    call free
.Lfl_done:
    leave
    ret

__a_print_string:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    mov QWORD PTR [rbp - 8], rdi
    test rdi, rdi
    je .Lps_done
    call strlen
    mov rsi, rax
    mov rdi, QWORD PTR [rbp - 8]
    call __a_out_raw
.Lps_done:
    leave
    ret

__a_print_int:
    push rbp
    mov rbp, rsp
    sub rsp, 96
    mov rcx, rdi
    lea rdi, [rbp - 80]
    mov esi, 64
    lea rdx, [rip + .La_fmt_i]
    xor eax, eax
    call snprintf
    cmp eax, 63
    jle .Lpi_len
    mov eax, 63
.Lpi_len:
    mov esi, eax
    lea rdi, [rbp - 80]
    call __a_out_raw
    leave
    ret

__a_print_double:
    push rbp
    mov rbp, rsp
    sub rsp, 96
    lea rdi, [rbp - 80]
    mov esi, 64
    lea rdx, [rip + .La_fmt_d]
    mov eax, 1
    call snprintf
    cmp eax, 63
    jle .Lpd_len
    mov eax, 63
.Lpd_len:
    mov esi, eax
    lea rdi, [rbp - 80]
    call __a_out_raw
    leave
    ret

__a_print_bool:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    test rdi, rdi
    je .Lpb_off
    lea rdi, [rip + .La_on]
    mov esi, 2
    call __a_out_raw
    leave
    ret
.Lpb_off:
    lea rdi, [rip + .La_off]
    mov esi, 3
    call __a_out_raw
    leave
    ret

__a_print_ptr:
    push rbp
    mov rbp, rsp
    sub rsp, 96
    mov rcx, rdi
    lea rdi, [rbp - 80]
    mov esi, 64
    lea rdx, [rip + .La_fmt_p]
    xor eax, eax
    call snprintf
    cmp eax, 63
    jle .Lpp_len
    mov eax, 63
.Lpp_len:
    mov esi, eax
    lea rdi, [rbp - 80]
    call __a_out_raw
    leave
    ret

__a_print_newline:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    lea rdi, [rip + .La_newline_bytes]
    mov esi, 1
    call __a_out_raw
    leave
    ret

# __a_print_cell(cell) -- print one tagged 16-byte cell
__a_print_cell:
    push rbp
    mov rbp, rsp
    sub rsp, 32
    mov QWORD PTR [rbp - 8], rdi
    mov rax, QWORD PTR [rdi]
    cmp rax, 0
    je .Lpc_int
    cmp rax, 1
    je .Lpc_double
    cmp rax, 2
    je .Lpc_string
    cmp rax, 3
    je .Lpc_bool
    cmp rax, 4
    je .Lpc_array
    lea rdi, [rip + .La_opaque]
    call __a_print_string
    leave
    ret
.Lpc_int:
    mov rdi, QWORD PTR [rbp - 8]
    mov rdi, QWORD PTR [rdi + 8]
    call __a_print_int
    leave
    ret
.Lpc_double:
    mov rax, QWORD PTR [rbp - 8]
    movsd xmm0, QWORD PTR [rax + 8]
    call __a_print_double
    leave
    ret
.Lpc_string:
    mov rdi, QWORD PTR [rbp - 8]
    mov rdi, QWORD PTR [rdi + 8]
    call __a_print_string
    leave
    ret
.Lpc_bool:
    mov rdi, QWORD PTR [rbp - 8]
    mov rdi, QWORD PTR [rdi + 8]
    call __a_print_bool
    leave
    ret
.Lpc_array:
    mov rdi, QWORD PTR [rbp - 8]
    mov rdi, QWORD PTR [rdi + 8]
    call __a_print_array
    leave
    ret

# __a_print_array(array)
__a_print_array:
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov QWORD PTR [rbp - 8], rdi
    lea rdi, [rip + .La_lbracket]
    call __a_print_string
    mov QWORD PTR [rbp - 16], 0
.Lpa_loop:
    mov rax, QWORD PTR [rbp - 8]
    mov rcx, QWORD PTR [rbp - 16]
    cmp rcx, QWORD PTR [rax]
    jge .Lpa_done
    test rcx, rcx
    jz .Lpa_first
    lea rdi, [rip + .La_comma]
    call __a_print_string
.Lpa_first:
    mov rax, QWORD PTR [rbp - 8]
    mov rdx, QWORD PTR [rax + 16]
    mov rcx, QWORD PTR [rbp - 16]
    shl rcx, 4
    add rdx, rcx
    mov rdi, rdx
    call __a_print_cell
    inc QWORD PTR [rbp - 16]
    jmp .Lpa_loop
.Lpa_done:
    lea rdi, [rip + .La_rbracket]
    call __a_print_string
    leave
    ret

# __a_print_result(result)
__a_print_result:
    push rbp
    mov rbp, rsp
    sub rsp, 32
    mov QWORD PTR [rbp - 8], rdi
    mov rax, QWORD PTR [rdi]
    test rax, rax
    jz .Lpr_err
    lea rdi, [rip + .La_ok_prefix]
    call __a_print_string
    jmp .Lpr_value
.Lpr_err:
    lea rdi, [rip + .La_err_prefix]
    call __a_print_string
    mov rax, QWORD PTR [rbp - 8]
    mov rdi, QWORD PTR [rax + 24]
    call __a_print_string
    leave
    ret
.Lpr_value:
    mov rax, QWORD PTR [rbp - 8]
    mov rcx, QWORD PTR [rax + 8]
    mov QWORD PTR [rbp - 24], rcx
    mov rcx, QWORD PTR [rax + 16]
    mov QWORD PTR [rbp - 16], rcx
    lea rdi, [rbp - 24]
    call __a_print_cell
    leave
    ret

# -------------------------------------------------------------------- arrays
# Layout: [length][capacity][data][pad]
__a_array_create:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    mov rdi, 32
    call __a_alloc
    mov QWORD PTR [rax], 0
    mov QWORD PTR [rax + 8], 0
    mov QWORD PTR [rax + 16], 0
    mov QWORD PTR [rax + 24], 0
    leave
    ret

__a_array_append:
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov QWORD PTR [rbp - 8], rdi
    mov QWORD PTR [rbp - 16], rsi
    mov QWORD PTR [rbp - 24], rdx
    mov rax, QWORD PTR [rdi]
    cmp rax, QWORD PTR [rdi + 8]
    jl .Laa_space
    mov rcx, QWORD PTR [rdi + 8]
    test rcx, rcx
    jnz .Laa_grow
    mov rcx, 4
    jmp .Laa_alloc
.Laa_grow:
    shl rcx, 1
.Laa_alloc:
    mov QWORD PTR [rbp - 32], rcx
    mov rdi, rcx
    shl rdi, 4
    call __a_alloc
    mov QWORD PTR [rbp - 40], rax
    mov r8, QWORD PTR [rbp - 8]
    mov rsi, QWORD PTR [r8 + 16]
    test rsi, rsi
    jz .Laa_no_old
    mov rdi, rax
    mov rdx, QWORD PTR [r8]
    shl rdx, 4
    call memcpy
    mov r8, QWORD PTR [rbp - 8]
    mov rdi, QWORD PTR [r8 + 16]
    call __a_release
.Laa_no_old:
    mov r8, QWORD PTR [rbp - 8]
    mov rax, QWORD PTR [rbp - 40]
    mov QWORD PTR [r8 + 16], rax
    mov rcx, QWORD PTR [rbp - 32]
    mov QWORD PTR [r8 + 8], rcx
.Laa_space:
    mov r8, QWORD PTR [rbp - 8]
    mov rcx, QWORD PTR [r8]
    mov rdx, QWORD PTR [r8 + 16]
    mov rsi, rcx
    shl rsi, 4
    add rdx, rsi
    mov rsi, QWORD PTR [rbp - 16]
    mov QWORD PTR [rdx], rsi
    mov rsi, QWORD PTR [rbp - 24]
    mov QWORD PTR [rdx + 8], rsi
    inc QWORD PTR [r8]
    # Append returns the array so appends can chain in a literal.
    mov rax, r8
    leave
    ret

__a_array_get:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    cmp rsi, 1
    jl __a_error_index
    cmp rsi, QWORD PTR [rdi]
    jg __a_error_index
    dec rsi
    shl rsi, 4
    mov rax, QWORD PTR [rdi + 16]
    add rax, rsi
    leave
    ret

__a_array_set:
    push rbp
    mov rbp, rsp
    sub rsp, 32
    mov QWORD PTR [rbp - 8], rdx
    mov QWORD PTR [rbp - 16], rcx
    call __a_array_get
    mov rcx, QWORD PTR [rbp - 8]
    mov QWORD PTR [rax], rcx
    mov rcx, QWORD PTR [rbp - 16]
    mov QWORD PTR [rax + 8], rcx
    leave
    ret

__a_array_remove:
    push rbp
    mov rbp, rsp
    sub rsp, 32
    mov QWORD PTR [rbp - 8], rdi
    mov QWORD PTR [rbp - 16], rsi
    cmp rsi, 1
    jl __a_error_index
    cmp rsi, QWORD PTR [rdi]
    jg __a_error_index
    dec rsi
    shl rsi, 4
    mov rdi, QWORD PTR [rdi + 16]
    add rdi, rsi
    lea rsi, [rdi + 16]
    mov rax, QWORD PTR [rbp - 8]
    mov rdx, QWORD PTR [rax]
    sub rdx, QWORD PTR [rbp - 16]
    shl rdx, 4
    test rdx, rdx
    jz .Lar_done
    call memmove
.Lar_done:
    mov rax, QWORD PTR [rbp - 8]
    dec QWORD PTR [rax]
    leave
    ret

# ------------------------------------------------------------------- structs
# Layout: [field count][pad][tag,value]*
__a_struct_new:
    push rbp
    mov rbp, rsp
    sub rsp, 32
    mov QWORD PTR [rbp - 8], rdi
    shl rdi, 4
    add rdi, 16
    call __a_alloc
    mov QWORD PTR [rbp - 16], rax
    mov rcx, QWORD PTR [rbp - 8]
    mov QWORD PTR [rax], rcx
    lea rdi, [rax + 16]
    xor esi, esi
    mov rdx, QWORD PTR [rbp - 8]
    shl rdx, 4
    call memset
    mov rax, QWORD PTR [rbp - 16]
    leave
    ret

__a_struct_set:
    shl rcx, 4
    lea rax, [rdi + rcx + 16]
    mov QWORD PTR [rax], rsi
    mov QWORD PTR [rax + 8], rdx
    ret

# ------------------------------------------------------------------- results
# Layout: [ok][tag][value][error]
__a_result_ok:
    push rbp
    mov rbp, rsp
    sub rsp, 32
    mov QWORD PTR [rbp - 8], rdi
    mov QWORD PTR [rbp - 16], rsi
    mov rdi, 32
    call __a_alloc
    mov QWORD PTR [rax], 1
    mov rcx, QWORD PTR [rbp - 8]
    mov QWORD PTR [rax + 8], rcx
    mov rcx, QWORD PTR [rbp - 16]
    mov QWORD PTR [rax + 16], rcx
    mov QWORD PTR [rax + 24], 0
    leave
    ret

__a_result_err:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    mov QWORD PTR [rbp - 8], rdi
    mov rdi, 32
    call __a_alloc
    mov QWORD PTR [rax], 0
    mov QWORD PTR [rax + 8], 0
    mov QWORD PTR [rax + 16], 0
    mov rcx, QWORD PTR [rbp - 8]
    mov QWORD PTR [rax + 24], rcx
    leave
    ret

# ------------------------------------------------------------------- strings
__a_str_eq:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    call strcmp
    test eax, eax
    sete al
    movzx rax, al
    leave
    ret

__a_str_len:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    call strlen
    leave
    ret

# __a_int_to_string(int) -> string (shared buffer; used only for messages)
__a_int_to_string:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    mov rsi, rdi
    lea rdi, [rip + __a_int_buf]
    lea rdx, [rip + .La_fmt_i]
    xchg rsi, rdx
    xor eax, eax
    call sprintf
    lea rax, [rip + __a_int_buf]
    leave
    ret

# -------------------------------------------------------------------- random
__a_random_range:
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov QWORD PTR [rbp - 8], rdi
    mov QWORD PTR [rbp - 16], rsi
    cmp rsi, rdi
    jl __a_error_random
    mov rax, 1
    lock xadd QWORD PTR [rip + __a_random_state], rax
    mov rcx, rax
    shr rcx, 30
    xor rax, rcx
    movabs rcx, 0xBF58476D1CE4E5B9
    imul rax, rcx
    mov rcx, rax
    shr rcx, 27
    xor rax, rcx
    movabs rcx, 0x94D049BB133111EB
    imul rax, rcx
    mov rcx, rax
    shr rcx, 31
    xor rax, rcx
    mov rcx, QWORD PTR [rbp - 16]
    sub rcx, QWORD PTR [rbp - 8]
    inc rcx
    xor rdx, rdx
    div rcx
    mov rax, rdx
    add rax, QWORD PTR [rbp - 8]
    leave
    ret

# ---------------------------------------------------------------- bit helpers
__a_clz:
    mov rax, rdi
    test rax, rax
    jz .Lclz_zero
    bsr rcx, rax
    mov rax, 63
    sub rax, rcx
    ret
.Lclz_zero:
    mov rax, 64
    ret

__a_ctz:
    mov rax, rdi
    test rax, rax
    jz .Lctz_zero
    bsf rcx, rax
    mov rax, rcx
    ret
.Lctz_zero:
    mov rax, 64
    ret

# --------------------------------------------------------- fatal diagnostics
# __a_fail_raw(message): print to stderr, exit 1
__a_fail_raw:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    mov QWORD PTR [rbp - 8], rdi
    mov rdi, r15
    call __a_buf_flush
    mov rdi, QWORD PTR [rbp - 8]
    mov rdx, rdi
    mov rax, QWORD PTR [rip + stderr]
    mov rdi, rax
    lea rsi, [rip + .La_fmt_err]
    xor eax, eax
    call fprintf
    mov edi, 1
    call exit

# __a_fail_exit(message): print "error: <message>", exit 1
__a_fail_exit:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    mov QWORD PTR [rbp - 8], rdi
    mov rdi, r15
    call __a_buf_flush
    mov rdi, QWORD PTR [rbp - 8]
    mov rdx, rdi
    mov rax, QWORD PTR [rip + stderr]
    mov rdi, rax
    lea rsi, [rip + .La_fmt_fail]
    xor eax, eax
    call fprintf
    mov edi, 1
    call exit

__a_error_index:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    lea rdi, [rip + .La_msg_index]
    call __a_fail_raw
    leave
    ret

__a_error_div_zero:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    lea rdi, [rip + .La_msg_div]
    call __a_fail_raw
    leave
    ret

__a_error_type:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    lea rdi, [rip + .La_msg_type]
    call __a_fail_raw
    leave
    ret

__a_error_random:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    lea rdi, [rip + .La_msg_random]
    call __a_fail_raw
    leave
    ret

__a_error_oom:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    lea rdi, [rip + .La_msg_oom]
    call __a_fail_raw
    leave
    ret
"#;

pub const RODATA: &str = r#"
.La_fmt_s: .asciz "%s"
.La_fmt_i: .asciz "%lld"
.La_fmt_d: .asciz "%.15g"
.La_fmt_p: .asciz "0x%llx"
.La_fmt_nl: .asciz "\n"
.La_newline_bytes: .ascii "\n"
.La_fmt_err: .asciz "%s"
.La_fmt_fail: .asciz "error: %s\n"
.La_on: .asciz "on"
.La_off: .asciz "off"
.La_lbracket: .asciz "["
.La_rbracket: .asciz "]"
.La_comma: .asciz ", "
.La_opaque: .asciz "<value>"
.La_ok_prefix: .asciz "ok "
.La_err_prefix: .asciz "failed: "
.La_neg_zero: .quad 0x8000000000000000
.La_msg_index: .asciz "error: array index out of bounds\n"
.La_msg_div: .asciz "error: division by zero\n"
.La_msg_type: .asciz "error: array element type mismatch\n"
.La_msg_random: .asciz "error: random range invalid (min > max)\n"
.La_msg_oom: .asciz "error: out of memory\n"
"#;

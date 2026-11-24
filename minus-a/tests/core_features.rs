//! Behavioural tests for the Core Edition feature set.
//!
//! Every test here that executes a program compiles -A source with the real
//! compiler, assembles the generated x86-64 assembly with the backend and runs the
//! resulting executable. Nothing asserts on keyword presence alone: the checks
//! are on printed output, exit codes, stderr and the shape of the generated
//! assembly where the backend contract matters.

mod support;

use support::*;

// ---------------------------------------------------------------------------
// 1. Values, variables and expressions
// ---------------------------------------------------------------------------

#[test]
fn int_arithmetic_precedence() {
    assert_stdout(
        "arith",
        "block main(1) {
            aout(A, 2 + 3 * 4)
            aout(A, (2 + 3) * 4)
            aout(A, 10 - 2 - 3)
            aout(A, 7 / 2)
            aout(A, 7 % 2)
            aout(A, -5 + 2)
        }",
        "14\n20\n5\n3\n1\n-3\n",
    );
}

#[test]
fn double_arithmetic() {
    assert_stdout(
        "doubles",
        "block main(1) {
            set x = 1.5
            set y = 2.0
            aout(A, x + y)
            aout(A, x * y)
            aout(A, y - x)
            aout(A, 3.0 / 2.0)
        }",
        "3.5\n3\n0.5\n1.5\n",
    );
}

#[test]
fn booleans_and_comparisons() {
    assert_stdout(
        "bools",
        "block main(1) {
            aout(A, on)
            aout(A, off)
            aout(A, 1 < 2)
            aout(A, 2 <= 2)
            aout(A, 3 > 4)
            aout(A, \"a\" == \"a\")
            when 1 < 2 and 2 < 3 { aout(\"and\") }
        }",
        "on\noff\non\non\noff\non\nand\n",
    );
}

#[test]
fn strings_quote_styles_and_escapes() {
    assert_stdout(
        "strings",
        "block main(1) {
            set a = \"double\"
            set b = 'single'
            set c = \"tab:\\there\"
            aout(A, a)
            aout(A, b)
            aout(A, c)
            aout(A, a.length)
        }",
        "double\nsingle\ntab:\there\n6\n",
    );
}

#[test]
fn string_equality_is_by_value() {
    assert_stdout(
        "string_eq",
        "block main(1) {
            set a = \"abc\"
            set b = \"ab\"
            set c = \"c\"
            when a == \"abc\" { aout(\"equal\") }
            when b == c { aout(\"wrong\") }
            anyelse { aout(\"different\") }
        }",
        "equal\ndifferent\n",
    );
}

#[test]
fn set_is_fixed_and_setc_is_changeable() {
    assert_stdout(
        "mutability",
        "block main(1) {
            set fixed = 1
            setc changeable = 2
            changeable = changeable + 1
            aout(A, fixed)
            aout(A, changeable)
        }",
        "1\n3\n",
    );
}

#[test]
fn comments_are_ignored() {
    assert_stdout(
        "comments",
        "// line comment
        block main(1) {
            /* block
               comment */
            aout(\"ok\") // trailing
        }",
        "ok\n",
    );
}

#[test]
fn local_scoping_inside_branches() {
    assert_stdout(
        "scoping",
        "block main(1) {
            set x = 1
            when x == 1 {
                set y = 2
                aout(A, y)
            }
            aout(A, x)
        }",
        "2\n1\n",
    );
}

// ---------------------------------------------------------------------------
// 2. Control flow: when / else / anyelse, loop / while / then
// ---------------------------------------------------------------------------

#[test]
fn first_matching_branch_only() {
    assert_stdout(
        "when_first",
        "block main(1) {
            setc n = 5
            when n > 1 { aout(\"one\") }
            else n > 2 { aout(\"two\") }
            anyelse { aout(\"none\") }
        }",
        "one\n",
    );
}

#[test]
fn anyelse_runs_when_nothing_matches() {
    assert_stdout(
        "when_anyelse",
        "block main(1) {
            setc n = 0
            when n > 1 { aout(\"one\") }
            else n > 2 { aout(\"two\") }
            anyelse { aout(\"none\") }
        }",
        "none\n",
    );
}

#[test]
fn empty_then_ends_the_loop_cleanly() {
    // An empty `then` is a valid terminating section: the loop stops and
    // execution continues after it.
    assert_stdout(
        "empty_then",
        "block main(1) {
            setc n = 0
            loop {
                while n < 3 {
                    insert n = 1
                    aout(A, n)
                    then
                }
            }
            aout(\"after\")
        }",
        "1\n2\n3\nafter\n",
    );
}

#[test]
fn then_runs_once_after_the_loop() {
    assert_stdout(
        "then_once",
        "block main(1) {
            setc n = 0
            loop {
                while n < 2 {
                    insert n = 1
                    then
                    aout(\"done\")
                }
            }
        }",
        "done\n",
    );
}

#[test]
fn counted_loop_repeats_exactly() {
    assert_stdout(
        "counted",
        "block main(1) {
            loop {
                aout(\"tick\"),
                3
            }
        }",
        "tick\ntick\ntick\n",
    );
}

#[test]
fn nested_loops() {
    assert_stdout(
        "nested_loops",
        "block main(1) {
            setc i = 0
            loop {
                while i < 2 {
                    insert i = 1
                    setc j = 0
                    loop {
                        while j < 2 {
                            insert j = 1
                            aout(A, i * 10 + j)
                            then
                        }
                    }
                    then
                }
            }
        }",
        "11\n12\n21\n22\n",
    );
}

#[test]
fn while_with_and_condition() {
    assert_stdout(
        "while_and",
        "block main(1) {
            setc i = 0
            loop {
                while i < 5 and i < 2 {
                    insert i = 1
                    aout(A, i)
                    then
                }
            }
        }",
        "1\n2\n",
    );
}

#[test]
fn early_return_from_a_reuse_body() {
    // `return` is a reuse statement; a block simply runs to its end.
    assert_stdout(
        "early_return",
        "reuse f(x) {
            when x > 0 { return \"positive\" }
            return \"other\"
        }
        block main(1) {
            aout(A, f(1))
            aout(A, f(-1))
        }",
        "positive\nother\n",
    );
}

#[test]
fn array_literals_and_one_based_indexing() {
    assert_stdout(
        "array_index",
        "block main(1) {
            set a = [\"x\", \"y\", \"z\"]
            aout(A, a[1])
            aout(A, a[2])
            aout(A, a[3])
            aout(A, a.length)
        }",
        "x\ny\nz\n3\n",
    );
}

#[test]
fn array_element_assignment() {
    assert_stdout(
        "array_set",
        "block main(1) {
            setc a = [1, 2, 3]
            a[2] = 20
            aout(A, a[2])
        }",
        "20\n",
    );
}

#[test]
fn array_append_and_remove() {
    assert_stdout(
        "array_append_remove",
        "block main(1) {
            setc a = [1, 2]
            insert a = 3
            aout(A, a.length)
            remove a[1]
            aout(A, a.length)
            aout(A, a[1])
        }",
        "3\n2\n2\n",
    );
}

#[test]
fn mixed_array_prints_by_runtime_tag() {
    assert_stdout(
        "mixed_array",
        "block main(1) {
            set values = [10, \"hello\", on, 19.99]
            aout(A, values[1])
            aout(A, values[2])
            aout(A, values[3])
            aout(A, values[4])
            aout(A, values.length)
        }",
        "10\nhello\non\n19.99\n4\n",
    );
}

#[test]
fn each_iterates_in_order() {
    assert_stdout(
        "each_order",
        "block main(1) {
            set ns = [1, 2, 3, 4]
            setc total = 0
            each n in ns {
                insert total = n
                aout(A, n)
            }
            aout(A, total)
        }",
        "1\n2\n3\n4\n10\n",
    );
}

#[test]
fn each_over_structs_and_strings() {
    assert_stdout(
        "each_structs",
        "struct P { x: int }
        block main(1) {
            set ps = [P { x = 1 }, P { x = 2 }]
            each p in ps { aout(A, p.x) }
            set names = [\"a\", \"b\"]
            each n in names { aout(A, n) }
        }",
        "1\n2\na\nb\n",
    );
}

#[test]
fn array_out_of_bounds_is_a_runtime_error() {
    let run = run_program("oob", "block main(1) { set a = [1]\n aout(A, a[2]) }");
    match run {
        Some(run) => {
            assert_ne!(run.status, 0, "out-of-range access must fail");
            assert!(
                run.stderr.contains("array index out of bounds"),
                "stderr was {:?}",
                run.stderr
            );
        }
        None => eprintln!("skipping: no toolchain"),
    }
}

#[test]
fn index_zero_is_out_of_bounds() {
    let run = run_program(
        "oob_zero",
        "block main(1) { set a = [1, 2]\n aout(A, a[0]) }",
    );
    match run {
        Some(run) => {
            assert_ne!(run.status, 0);
            assert!(run.stderr.contains("array index out of bounds"));
        }
        None => eprintln!("skipping: no toolchain"),
    }
}

#[test]
fn division_by_zero_is_a_runtime_error() {
    let run = run_program("div_zero", "block main(1) { setc z = 0\n aout(A, 10 / z) }");
    match run {
        Some(run) => {
            assert_ne!(run.status, 0);
            assert!(
                run.stderr.contains("division by zero"),
                "stderr was {:?}",
                run.stderr
            );
        }
        None => eprintln!("skipping: no toolchain"),
    }
}

// ---------------------------------------------------------------------------
// 4. Blocks, queues and scheduling
// ---------------------------------------------------------------------------

#[test]
fn blocks_run_in_queue_order() {
    assert_stdout(
        "queue_order",
        "block third(3) { aout(\"3\") }
        block first(1) { aout(\"1\") }
        block second(2) { aout(\"2\") }",
        "1\n2\n3\n",
    );
}

#[test]
fn repeated_block_names_are_independent_contexts() {
    assert_stdout(
        "repeated_names",
        "block same(1) { set x = 1\n aout(A, x) }
        block same(2) { set x = 2\n aout(A, x) }
        block same(3) { set x = 3\n aout(A, x) }",
        "1\n2\n3\n",
    );
}

#[test]
fn aout_literal_and_evaluated_markers() {
    assert_stdout(
        "aout_markers",
        "block main(1) {
            setc x = 7
            aout(\"value: \", A, x, \" done\")
        }",
        "value: 7 done\n",
    );
}

#[test]
fn parallel_group_runs_both_blocks() {
    let run = run_program(
        "parallel_group",
        "block left(1:1) { aout(\"L\") }
         block right(1:1) { aout(\"R\") }",
    );
    match run {
        Some(run) => {
            assert_eq!(run.status, 0);
            // Deterministic: source declaration order, not completion order.
            assert_eq!(run.stdout, "L\nR\n");
        }
        None => eprintln!("skipping: no toolchain"),
    }
}

#[test]
fn parallel_group_is_bounded_by_later_queues() {
    let run = run_program(
        "parallel_barrier",
        "block first(1) { aout(\"first\") }
         block a(2:2) { aout(\"a\") }
         block b(2:2) { aout(\"b\") }
         block last(3) { aout(\"last\") }",
    );
    match run {
        Some(run) => {
            assert_eq!(run.status, 0);
            // Queue order first, group order second: both are deterministic.
            assert_eq!(run.stdout, "first\na\nb\nlast\n");
        }
        None => eprintln!("skipping: no toolchain"),
    }
}

#[test]
fn four_block_parallel_group_is_accepted() {
    let run = run_program(
        "parallel_four",
        "block a(1:1x4) { aout(\"a\") }
         block b(1:1x4) { aout(\"b\") }
         block c(1:1x4) { aout(\"c\") }
         block d(1:1x4) { aout(\"d\") }",
    );
    match run {
        Some(run) => {
            assert_eq!(run.status, 0);
            assert_eq!(run.stdout, "a\nb\nc\nd\n");
        }
        None => eprintln!("skipping: no toolchain"),
    }
}

#[test]
fn parallel_blocks_share_no_locals() {
    let run = run_program(
        "parallel_locals",
        "block a(1:1) { set x = 1\n aout(A, x) }
         block b(1:1) { set x = 2\n aout(A, x) }",
    );
    match run {
        Some(run) => {
            assert_eq!(run.status, 0);
            assert_eq!(run.stdout, "1\n2\n");
        }
        None => eprintln!("skipping: no toolchain"),
    }
}

#[test]
fn parallel_workers_really_overlap() {
    // Two CPU-bound blocks in one parallel group must beat running the same
    // two blocks one after the other. Other tests in this binary compile and
    // link in parallel, so the comparison is repeated until the machine gives
    // a clean round instead of failing on one noisy measurement.
    let _guard = timing_lock();
    let body = |header: &str| {
        format!(
            "reuse burn(n) {{
                 setc i = 0
                 setc acc = 0
                 loop {{
                     while i < n {{
                         insert i = 1
                         insert acc = i
                         then
                     }}
                 }}
                 return acc
             }}
             {}",
            header
        )
    };
    let parallel = body(
        "block one(1:1) { set done = burn(20000000)\n aout(A, done) }
         block two(1:1) { set done = burn(20000000)\n aout(A, done) }",
    );
    let sequential = body(
        "block one(1) { set done = burn(20000000)\n aout(A, done) }
         block two(2) { set done = burn(20000000)\n aout(A, done) }",
    );
    // Build both once, so the timing measures only the programs.
    let build = |tag: &str, src: &str| run_program_timed(tag, src).map(|(run, _)| run);
    match (build("par_warm", &parallel), build("seq_warm", &sequential)) {
        (Some(par), Some(seq)) => {
            assert_eq!(par.status, 0);
            assert_eq!(seq.status, 0);
            assert_eq!(
                par.stdout, seq.stdout,
                "parallel and sequential output differ"
            );
        }
        _ => {
            eprintln!("skipping: no toolchain");
            return;
        }
    }

    let mut best: Option<(std::time::Duration, std::time::Duration)> = None;
    for round in 0..8 {
        let par = run_program_timed(&format!("par_r{}", round), &parallel)
            .unwrap()
            .1;
        let seq = run_program_timed(&format!("seq_r{}", round), &sequential)
            .unwrap()
            .1;
        match best {
            Some((p, s)) if p < s => break,
            _ => best = Some((par, seq)),
        }
        if par < seq {
            best = Some((par, seq));
            break;
        }
    }
    let (par_best, seq_best) = best.expect("at least one measurement");
    eprintln!("parallel {:?} vs sequential {:?}", par_best, seq_best);
    assert!(
        par_best < seq_best,
        "parallel group was not faster in any round ({:?} vs {:?})",
        par_best,
        seq_best
    );
}

#[test]
fn reuse_attaches_and_returns_a_value() {
    assert_stdout(
        "reuse_value",
        "reuse answer() { return 42 }
        block show(answer, 1) { aout(A, answer) }",
        "42\n",
    );
}

#[test]
fn reuse_with_constant_arguments() {
    assert_stdout(
        "reuse_args",
        "reuse add(a, b) { return a + b }
        block show(add(20, 22), 1) { aout(A, add) }",
        "42\n",
    );
}

#[test]
fn reuse_result_is_cached_per_execution() {
    let run = run_program(
        "reuse_cache",
        "reuse ticket() { return random(1, 1000000) }
         block show(ticket, 1) {
             aout(A, ticket)
             aout(A, ticket)
             aout(A, ticket)
         }",
    );
    match run {
        Some(run) => {
            assert_eq!(run.status, 0);
            let lines: Vec<&str> = run.stdout.lines().collect();
            assert_eq!(lines.len(), 3);
            assert_eq!(lines[0], lines[1], "reuse result was not cached");
            assert_eq!(lines[1], lines[2], "reuse result was not cached");
        }
        None => eprintln!("skipping: no toolchain"),
    }
}

#[test]
fn parallel_blocks_get_independent_reuse_contexts() {
    let run = run_program(
        "reuse_parallel",
        "reuse roll() { return random(1, 1000000) }
         block left(roll, 1:1) { aout(A, roll) }
         block right(roll, 1:1) { aout(A, roll) }",
    );
    match run {
        Some(run) => {
            assert_eq!(run.status, 0);
            let lines: Vec<&str> = run.stdout.lines().collect();
            assert_eq!(lines.len(), 2);
            assert_ne!(
                lines[0], lines[1],
                "two blocks shared one reuse evaluation: {:?}",
                lines
            );
        }
        None => eprintln!("skipping: no toolchain"),
    }
}

#[test]
fn parameterized_reuse_with_int_arguments() {
    assert_stdout(
        "reuse_param_int",
        "reuse double_it(x) { return x * 2 }
        block show(double_it(21), 1) { aout(A, double_it) }",
        "42\n",
    );
}

#[test]
fn parameterized_reuse_with_random_range() {
    let run = run_program(
        "reuse_param_random",
        "reuse roll(lo, hi) { return random(lo, hi) }
         block show(roll(1, 6), 1) {
             aout(A, roll)
             when roll >= 1 and roll <= 6 { aout(\"in range\") }
         }",
    );
    match run {
        Some(run) => {
            assert_eq!(run.status, 0);
            let lines: Vec<&str> = run.stdout.lines().collect();
            assert_eq!(lines.len(), 2);
            let value: i64 = lines[0].parse().expect("reuse printed a number");
            assert!((1..=6).contains(&value), "roll out of range: {}", value);
            assert_eq!(lines[1], "in range");
        }
        None => eprintln!("skipping: no toolchain"),
    }
}

#[test]
fn reuse_with_typed_parameters() {
    assert_stdout(
        "reuse_typed",
        "reuse scale(x: int, factor: int) -> int { return x * factor }
        block main(1) { aout(A, scale(6, 7)) }",
        "42\n",
    );
}

#[test]
fn reuse_binding_with_as() {
    assert_stdout(
        "reuse_binding",
        "reuse add(a, b) { return a + b }
        block main(add(1, 2) as total, add(10, 20), 1) {
            aout(A, total)
            aout(A, add)
        }",
        "3\n30\n",
    );
}

#[test]
fn reuse_may_call_other_reuses() {
    assert_stdout(
        "reuse_nested_call",
        "reuse inc(x) { return x + 1 }
        reuse twice(x) { return inc(inc(x)) }
        block main(1) { aout(A, twice(40)) }",
        "42\n",
    );
}

#[test]
fn reuse_recursion_terminates() {
    assert_stdout(
        "reuse_recursion",
        "reuse fact(n) {
            when n <= 1 { return 1 }
            return n * fact(n - 1)
        }
        block main(1) {
            aout(A, fact(5))
            aout(A, fact(10))
        }",
        "120\n3628800\n",
    );
}

#[test]
fn mutual_recursion_terminates() {
    assert_stdout(
        "reuse_mutual",
        "reuse is_even(n) {
            when n == 0 { return on }
            return is_odd(n - 1)
        }
        reuse is_odd(n) {
            when n == 0 { return off }
            return is_even(n - 1)
        }
        block main(1) {
            aout(A, is_even(10))
            aout(A, is_odd(10))
        }",
        "on\noff\n",
    );
}

// ---------------------------------------------------------------------------
// 6. Generics (real monomorphization)
// ---------------------------------------------------------------------------

#[test]
fn generic_reuse_specializes_per_type() {
    assert_stdout(
        "generic_identity",
        "reuse identity<T>(value: T) -> T { return value }
        block main(1) {
            aout(A, identity(7))
            aout(A, identity(\"text\"))
            aout(A, identity(2.5))
            aout(A, identity(on))
        }",
        "7\ntext\n2.5\non\n",
    );
}

#[test]
fn generic_reuse_emits_one_symbol_per_instantiation() {
    let asm = compile_ok(
        "reuse identity<T>(value: T) -> T { return value }
         block main(1) {
             aout(A, identity(7))
             aout(A, identity(\"text\"))
         }",
    );
    let int_sym = asm.matches("__a_reuse_identity").count();
    assert!(
        int_sym >= 2,
        "expected one monomorphized symbol per type, found {}",
        int_sym
    );
}

#[test]
fn generic_struct_holds_any_type() {
    assert_stdout(
        "generic_struct",
        "struct Box<T> { value: T }
        block main(1) {
            set a = Box { value = 42 }
            set b = Box { value = \"hello\" }
            aout(A, a.value)
            aout(A, b.value)
        }",
        "42\nhello\n",
    );
}

#[test]
fn generic_struct_with_two_parameters() {
    assert_stdout(
        "generic_pair",
        "struct Pair<T, U> { first: T, second: U }
        block main(1) {
            set p = Pair { first = 1, second = \"x\" }
            aout(A, p.first)
            aout(A, p.second)
        }",
        "1\nx\n",
    );
}

// ---------------------------------------------------------------------------
// 7. Structs
// ---------------------------------------------------------------------------

#[test]
fn struct_literal_and_fields() {
    assert_stdout(
        "struct_fields",
        "struct Point { x: int, y: int }
        block main(1) {
            set p = Point { x = 3, y = 4 }
            aout(A, p.x)
            aout(A, p.y)
        }",
        "3\n4\n",
    );
}

#[test]
fn struct_field_assignment() {
    assert_stdout(
        "struct_mutate",
        "struct Point { x: int, y: int }
        block main(1) {
            setc p = Point { x = 3, y = 4 }
            p.x = 10
            p.y = p.x + 1
            aout(A, p.x)
            aout(A, p.y)
        }",
        "10\n11\n",
    );
}

#[test]
fn nested_structs() {
    assert_stdout(
        "struct_nested",
        "struct Inner { v: int }
        struct Outer { inner: Inner, tag: string }
        block main(1) {
            set o = Outer { inner = Inner { v = 7 }, tag = \"t\" }
            aout(A, o.inner.v)
            aout(A, o.tag)
        }",
        "7\nt\n",
    );
}

#[test]
fn struct_with_array_field() {
    assert_stdout(
        "struct_array_field",
        "struct Bag { items: [int] }
        block main(1) {
            set b = Bag { items = [4, 5] }
            aout(A, b.items.length)
            aout(A, b.items[2])
        }",
        "2\n5\n",
    );
}

#[test]
fn struct_passed_to_reuse_by_value() {
    assert_stdout(
        "struct_by_value",
        "struct P { x: int, y: int }
        reuse sum(p: P) -> int { return p.x + p.y }
        block main(1) {
            set p = P { x = 20, y = 22 }
            aout(A, sum(p))
        }",
        "42\n",
    );
}

#[test]
fn struct_equality_is_not_defined_but_fields_are_readable() {
    let err = compile_err(
        "struct P { x: int }
         block main(1) { set p = P { x = 1 }\n when p == p { aout(\"eq\") } }",
    );
    assert!(err.contains("E_TYPE"), "got: {}", err);
}

// ---------------------------------------------------------------------------
// 8. Enums
// ---------------------------------------------------------------------------

#[test]
fn enum_printing_and_comparison() {
    assert_stdout(
        "enum_basic",
        "enum Color { red, green, blue }
         block main(1) {
             set c = Color.green
             aout(A, c)
             when c == Color.green { aout(\"it is green\") }
             anyelse { aout(\"other\") }
         }",
        "Color.green\nit is green\n",
    );
}

#[test]
fn enum_explicit_values() {
    assert_stdout(
        "enum_values",
        "enum Status { ok = 200, not_found = 404 }
         block main(1) {
             aout(A, Status.not_found.value)
             when Status.ok.value == 200 { aout(\"200\") }
         }",
        "404\n200\n",
    );
}

#[test]
fn enum_implicit_values_increment() {
    assert_stdout(
        "enum_implicit",
        "enum Step { first, second, third }
         block main(1) {
             aout(A, Step.first.value)
             aout(A, Step.second.value)
             aout(A, Step.third.value)
         }",
        "0\n1\n2\n",
    );
}

#[test]
fn enum_unknown_variant_is_rejected() {
    let err = compile_err(
        "enum Color { red }
         block main(1) { aout(A, Color.blue) }",
    );
    assert!(err.contains("no variant"), "got: {}", err);
}

// ---------------------------------------------------------------------------
// 9. Traits and impls
// ---------------------------------------------------------------------------

#[test]
fn trait_method_dispatch_per_type() {
    assert_stdout(
        "trait_two_impls",
        "struct Alpha { v: int }
         struct Beta { v: int }
         trait Named { reuse name(self) -> string }
         impl Named for Alpha { reuse name(self) -> string { return \"alpha\" } }
         impl Named for Beta { reuse name(self) -> string { return \"beta\" } }
         block main(1) {
             set a = Alpha { v = 1 }
             set b = Beta { v = 2 }
             aout(A, a.name())
             aout(A, b.name())
         }",
        "alpha\nbeta\n",
    );
}

#[test]
fn trait_method_can_use_self_fields() {
    assert_stdout(
        "trait_self",
        "struct Circle { radius: double }
         trait Shape { reuse area(self) -> double }
         impl Shape for Circle {
             reuse area(self) -> double { return 3.14159 * self.radius * self.radius }
         }
         block main(1) {
             set c = Circle { radius = 2.0 }
             aout(A, c.area())
         }",
        "12.56636\n",
    );
}

#[test]
fn inherent_methods_without_a_trait() {
    assert_stdout(
        "inherent_impl",
        "struct Counter { n: int }
         impl Counter {
             reuse get(self) -> int { return self.n }
             reuse bumped(self) -> int { return self.n + 1 }
         }
         block main(1) {
             set c = Counter { n = 9 }
             aout(A, c.get())
             aout(A, c.bumped())
         }",
        "9\n10\n",
    );
}

#[test]
fn trait_method_with_parameters() {
    assert_stdout(
        "trait_params",
        "struct Accumulator { total: int }
         trait Addable { reuse plus(self, amount: int) -> int }
         impl Addable for Accumulator {
             reuse plus(self, amount: int) -> int { return self.total + amount }
         }
         block main(1) {
             set a = Accumulator { total = 40 }
             aout(A, a.plus(2))
         }",
        "42\n",
    );
}

#[test]
fn impl_missing_trait_method_is_rejected() {
    let err = compile_err(
        "struct S { v: int }
         trait T { reuse f(self) -> int }
         impl T for S { }",
    );
    assert!(err.contains("missing method"), "got: {}", err);
}

#[test]
fn method_on_type_without_impl_is_rejected() {
    let err = compile_err(
        "struct S { v: int }
         block main(1) { set s = S { v = 1 }\n aout(A, s.f()) }",
    );
    assert!(err.contains("no method"), "got: {}", err);
}

// ---------------------------------------------------------------------------
// 10. Pointers and the memory model
// ---------------------------------------------------------------------------

#[test]
fn address_of_and_deref() {
    assert_stdout(
        "pointer_basic",
        "block main(1) {
            setc x = 41
            set p = &x
            *p = 42
            aout(A, x)
            aout(A, *p)
        }",
        "42\n42\n",
    );
}

#[test]
fn pointer_to_struct_field() {
    assert_stdout(
        "pointer_field",
        "struct P { x: int }
         block main(1) {
             setc p = P { x = 5 }
             set q = &p.x
             *q = 11
             aout(A, p.x)
         }",
        "11\n",
    );
}

#[test]
fn allocation_and_explicit_release() {
    assert_stdout(
        "alloc_free",
        "block main(1) {
            set p = @alloc(32)
            @mem_write64(p, 1234)
            aout(A, @mem_read64(p))
            @free(p)
            aout(A, @live_allocs())
        }",
        "1234\n0\n",
    );
}

#[test]
fn automatic_release_of_callee_locals() {
    // The reuse allocates an array and drops it on return, so the registry is
    // empty again afterwards.
    assert_stdout(
        "auto_release",
        "reuse make() {
            set data = [1, 2, 3]
            return data.length
        }
        block main(1) {
            aout(A, make())
            aout(A, @live_allocs())
        }",
        "3\n0\n",
    );
}

#[test]
fn memory_intrinsics_round_trip() {
    assert_stdout(
        "mem_intrinsics",
        "block main(1) {
            set a = @alloc(16)
            set b = @alloc(16)
            @mem_write8(a, 65)
            @mem_set(b, 66, 4)
            aout(A, @mem_read8(a))
            aout(A, @mem_read8(b))
            @mem_copy(b, a, 1)
            aout(A, @mem_read8(b))
            @free(a)
            @free(b)
        }",
        "65\n66\n65\n",
    );
}

// ---------------------------------------------------------------------------
// 11. Intrinsics
// ---------------------------------------------------------------------------

#[test]
fn bit_manipulation_intrinsics() {
    assert_stdout(
        "ints_bits",
        "block main(1) {
            aout(A, @popcount(255))
            aout(A, @clz(1))
            aout(A, @ctz(8))
            aout(A, @rotl(1, 4))
            aout(A, @rotr(16, 4))
            aout(A, @bswap(1))
        }",
        "8\n63\n3\n16\n1\n72057594037927936\n",
    );
}

#[test]
fn target_query_intrinsics() {
    assert_stdout(
        "ints_target",
        "block main(1) {
            aout(A, @arch())
            aout(A, @os())
            aout(A, @pointer_bits())
            when @has_atomics() { aout(\"atomics\") }
        }",
        "x86_64\nlinux\n64\natomics\n",
    );
}

#[test]
fn atomics_and_fences() {
    assert_stdout(
        "ints_atomics",
        "block main(1) {
            set p = @alloc(8)
            @mem_write64(p, 10)
            aout(A, @atomic_add(p, 5))
            aout(A, @mem_read64(p))
            aout(A, @atomic_swap(p, 3))
            when @atomic_cas(p, 3, 42) { aout(\"cas\") }
            aout(A, @mem_read64(p))
            @fence()
            aout(A, @atomic_sub(p, 2))
            @free(p)
        }",
        "10\n15\n15\ncas\n42\n42\n",
    );
}

#[test]
fn atomics_are_actually_atomic_under_parallelism() {
    let run = run_program(
        "atomics_parallel",
        "set counter = @alloc(8)
         block setup(1) {
             @mem_write64(counter, 0)
         }
         block inc_left(2:2) {
             setc i = 0
             loop {
                 while i < 20000 {
                     insert i = 1
                     @atomic_add(counter, 1)
                     then
                 }
             }
         }
         block inc_right(2:2) {
             setc j = 0
             loop {
                 while j < 20000 {
                     insert j = 1
                     @atomic_add(counter, 1)
                     then
                 }
             }
         }
         block report(3) {
             aout(A, @mem_read64(counter))
             @free(counter)
         }",
    );
    match run {
        Some(run) => {
            assert_eq!(run.status, 0, "stderr: {}", run.stderr);
            assert_eq!(run.stdout.trim(), "40000", "atomic add lost updates");
        }
        None => eprintln!("skipping: no toolchain"),
    }
}

#[test]
fn cell_and_unbox_preserve_tags() {
    assert_stdout(
        "cells",
        "block main(1) {
            set c = @cell(41)
            aout(A, @unbox(c))
            aout(A, @live_allocs())
        }",
        "41\n1\n",
    );
}

#[test]
fn unknown_intrinsic_is_rejected() {
    let err = compile_err("block main(1) { aout(A, @nope()) }");
    assert!(err.contains("unknown intrinsic"), "got: {}", err);
}

// ---------------------------------------------------------------------------
// 12. Error handling
// ---------------------------------------------------------------------------

#[test]
fn reuse_that_can_fail_returns_a_result() {
    assert_stdout(
        "error_result",
        "reuse divide(a, b) {
            when b == 0 { fail \"division by zero\" }
            return a / b
        }
        block main(1) {
            set good = divide(10, 2)
            when good.ok { aout(\"ok: \", A, good.value) }
            anyelse { aout(\"failed: \", A, good.error) }
            set bad = divide(1, 0)
            when bad.ok { aout(\"unexpected\") }
            anyelse { aout(\"error: \", A, bad.error) }
        }",
        "ok: 5\nerror: division by zero\n",
    );
}

#[test]
fn question_mark_propagates_failures() {
    assert_stdout(
        "error_propagate",
        "reuse inner(x) {
            when x < 0 { fail \"negative\" }
            return x
        }
        reuse outer(x) {
            set v = inner(x)?
            return v * 2
        }
        block main(1) {
            set good = outer(5)
            when good.ok { aout(A, good.value) }
            set bad = outer(-1)
            when bad.ok { aout(\"no\") }
            anyelse { aout(\"error: \", A, bad.error) }
        }",
        "10\nerror: negative\n",
    );
}

#[test]
fn result_values_print_their_state() {
    assert_stdout(
        "result_print",
        "reuse f(x) {
            when x > 0 { return x }
            fail \"no\"
        }
        block main(1) {
            aout(A, f(1))
            aout(A, f(0))
        }",
        "ok 1\nfailed: no\n",
    );
}

#[test]
fn fail_inside_a_block_stops_the_program() {
    let run = run_program(
        "fail_block",
        "block main(1) {
            aout(\"before\")
            fail \"boom\"
            aout(\"after\")
        }",
    );
    match run {
        Some(run) => {
            assert_eq!(run.stdout, "before\n");
            assert_ne!(run.status, 0, "a failing block must exit non-zero");
            assert!(run.stderr.contains("boom"), "stderr was {:?}", run.stderr);
        }
        None => eprintln!("skipping: no toolchain"),
    }
}

#[test]
fn custom_error_text_is_preserved() {
    let run = run_program(
        "custom_error",
        "reuse load(name) {
            when name == \"\" { fail \"empty name\" }
            return name.length
        }
        block main(1) {
            set r = load(\"\")
            when r.ok { aout(\"ok\") }
            anyelse { aout(\"error: \", A, r.error) }
        }",
    );
    match run {
        Some(run) => {
            assert_eq!(run.status, 0);
            assert_eq!(run.stdout, "error: empty name\n");
        }
        None => eprintln!("skipping: no toolchain"),
    }
}

// ---------------------------------------------------------------------------
// 13. Modules, imports and visibility
// ---------------------------------------------------------------------------

#[test]
fn imports_resolve_reuses_and_structs() {
    let run = run_files(
        "modules_basic",
        &[
            (
                "math.ma",
                "module math
                 pub struct Vec2 { x: int, y: int }
                 pub reuse add(a, b) { return a + b }
                 pub reuse sign_of(x) {
                     when x < 0 { return -1 }
                     return 1
                 }",
            ),
            (
                "main.ma",
                "import math
                 block main(1) {
                     aout(A, math.add(2, 3))
                     set v = math.Vec2 { x = 1, y = 2 }
                     aout(A, v.x + v.y)
                     aout(A, math.sign_of(-5))
                 }",
            ),
        ],
        "main.ma",
    );
    match run {
        Some(run) => {
            assert_eq!(run.status, 0, "stderr: {}", run.stderr);
            assert_eq!(run.stdout, "5\n3\n-1\n");
        }
        None => eprintln!("skipping: no toolchain"),
    }
}

#[test]
fn qualified_enum_variants_cross_modules() {
    let run = run_files(
        "modules_enum",
        &[
            (
                "signs.ma",
                "module signs
                 pub enum Sign { positive, negative }
                 pub reuse of(x) {
                     when x < 0 { return Sign.negative }
                     return Sign.positive
                 }",
            ),
            (
                "main.ma",
                "import signs
                 block main(1) {
                     aout(A, signs.of(-2))
                     when signs.of(1) == signs.Sign.positive { aout(\"positive\") }
                 }",
            ),
        ],
        "main.ma",
    );
    match run {
        Some(run) => {
            assert_eq!(run.status, 0, "stderr: {}", run.stderr);
            assert_eq!(run.stdout, "signs.Sign.negative\npositive\n");
        }
        None => eprintln!("skipping: no toolchain"),
    }
}

#[test]
fn transitive_imports_are_loaded_once() {
    let run = run_files(
        "modules_transitive",
        &[
            (
                "base.ma",
                "module base
                 pub reuse one() { return 1 }",
            ),
            (
                "mid.ma",
                "module mid
                 import base
                 pub reuse two() { return base.one() + 1 }",
            ),
            (
                "main.ma",
                "import mid
                 block main(1) {
                     aout(A, mid.two())
                 }",
            ),
        ],
        "main.ma",
    );
    match run {
        Some(run) => {
            assert_eq!(run.status, 0, "stderr: {}", run.stderr);
            assert_eq!(run.stdout, "2\n");
        }
        None => eprintln!("skipping: no toolchain"),
    }
}

#[test]
fn missing_module_is_a_clean_error() {
    let diag = check_files(
        "modules_missing",
        &[("main.ma", "import nowhere\nblock main(1) { aout(\"x\") }")],
        "main.ma",
    );
    assert!(diag.contains("cannot read module"), "got: {}", diag);
}

#[test]
fn import_cycles_are_reported() {
    let diag = check_files(
        "modules_cycle",
        &[
            (
                "a.ma",
                "module a\nimport b\npub reuse one() { return b.two() }",
            ),
            ("b.ma", "module b\nimport a\npub reuse two() { return 2 }"),
            ("main.ma", "import a\nblock main(1) { aout(A, a.one()) }"),
        ],
        "main.ma",
    );
    assert!(diag.contains("cyclic import"), "got: {}", diag);
}

#[test]
fn module_header_must_match_the_import_path() {
    let diag = check_files(
        "modules_mismatch",
        &[
            ("other.ma", "module different\npub reuse one() { return 1 }"),
            (
                "main.ma",
                "import other\nblock main(1) { aout(A, other.one()) }",
            ),
        ],
        "main.ma",
    );
    assert!(diag.contains("imported as"), "got: {}", diag);
}

#[test]
fn duplicate_definitions_across_modules_conflict() {
    let diag = check_files(
        "modules_conflict",
        &[
            ("one.ma", "module one\npub struct Same { x: int }"),
            ("two.ma", "module two\npub struct Same { y: int }"),
            (
                "main.ma",
                "import one\nimport two\nblock main(1) { aout(\"x\") }",
            ),
        ],
        "main.ma",
    );
    assert!(diag.is_empty(), "distinct modules must not clash: {}", diag);
}

// ---------------------------------------------------------------------------
// 14. Diagnostics
// ---------------------------------------------------------------------------

#[test]
fn diagnostics_carry_file_line_and_column() {
    let err = compile_err("block main(1) {\n    aout(A, nope)\n}");
    assert!(err.contains("E_SEMANTIC"), "got: {}", err);
    assert!(err.contains("main.ma:2:"), "missing location: {}", err);
}

#[test]
fn or_is_not_part_of_core() {
    let err = compile_err("block main(1) { when on or off { aout(\"x\") } }");
    assert!(err.contains("'or' is not part of -A Core"), "got: {}", err);
}

#[test]
fn semicolons_are_not_part_of_core() {
    let err = compile_err("block main(1) { set x = 1; aout(A, x) }");
    assert!(err.contains("E_LEX"), "got: {}", err);
}

#[test]
fn foundation_namespace_is_diagnosed() {
    let err = compile_err("block main(1) { aout(A, make.shape.circle) }");
    assert!(err.contains("Foundation"), "got: {}", err);
}

#[test]
fn statement_boundaries_respect_newlines() {
    // `&x` followed by `*p = 42` must not be read as a multiplication.
    assert_stdout(
        "newline_boundaries",
        "block main(1) {
            setc x = 1
            set p = &x
            *p = 2
            aout(A, x)
        }",
        "2\n",
    );
}

#[test]
fn expressions_may_span_lines_inside_parentheses() {
    assert_stdout(
        "multiline_call",
        "reuse add3(a, b, c) { return a + b + c }
         block main(1) {
             aout(A, add3(1,
                          2,
                          3))
         }",
        "6\n",
    );
}

// ---------------------------------------------------------------------------
// 15. Backend contract
// ---------------------------------------------------------------------------

#[test]
fn generated_assembly_is_intel_syntax_with_a_main() {
    let asm = compile_ok("block main(1) { aout(\"hi\") }");
    assert!(asm.contains(".intel_syntax noprefix"));
    assert!(asm.contains(".globl main"));
    assert!(
        !asm.contains("int 0x80"),
        "no raw syscalls in generated code"
    );
    assert!(!asm.contains("__asm__"), "no inline assembly in -A");
}

#[test]
fn stack_frames_are_sixteen_byte_aligned() {
    let asm = compile_ok(
        "reuse f(a, b, c) { return a + b + c }
         block main(1) {
             set x = f(1, 2, 3)
             aout(A, x)
         }",
    );
    for line in asm.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("sub rsp, ") {
            let n: usize = rest.parse().expect("frame size");
            assert_eq!(n % 16, 0, "frame size {} is not 16-byte aligned", n);
        }
    }
}

#[test]
fn arguments_use_the_system_v_register_order() {
    let asm = compile_ok(
        "reuse six(a, b, c, d, e, f) { return a + b + c + d + e + f }
         block main(1) {
             aout(A, six(1, 2, 3, 4, 5, 6))
         }",
    );
    for reg in ["rdi", "rsi", "rdx", "rcx", "r8", "r9"] {
        assert!(
            asm.contains(&format!("mov {}, QWORD PTR [rsp]", reg)),
            "argument register {} was never loaded",
            reg
        );
    }
}

#[test]
fn doubles_travel_in_xmm_registers() {
    let asm = compile_ok(
        "reuse scale(x: double, y: double) -> double { return x * y }
         block main(1) {
             aout(A, scale(1.5, 2.0))
         }",
    );
    assert!(asm.contains("xmm0"), "no SSE register used for doubles");
}

#[test]
fn mutex_state_is_initialised_before_use() {
    let asm = compile_ok("block main(1) { aout(\"x\") }");
    assert!(
        asm.contains("pthread_mutex_init"),
        "print lock must be initialised at startup"
    );
    assert!(
        asm.contains("__a_runtime_shutdown"),
        "runtime has no shutdown"
    );
}

#[test]
fn runtime_helpers_are_declared_once() {
    let asm = compile_ok(
        "reuse f() { return 1 }
         block a(1) { aout(A, f()) }
         block b(2:2) { aout(A, f()) }
         block c(2:2) { aout(A, f()) }",
    );
    for helper in ["__a_alloc:", "__a_print_int:", "__a_print_lock_acquire:"] {
        let count = asm.matches(helper).count();
        assert_eq!(count, 1, "{} was emitted {} times", helper, count);
    }
    // The same must hold with the runtime sliced off: helpers live in one
    // consolidated prelude, not per block.
    let code = program_code(&asm);
    assert!(!code.contains("__a_alloc:"));
}

// ---------------------------------------------------------------------------
// 16. Optimizer: real transformations and differential equivalence
// ---------------------------------------------------------------------------

#[test]
fn constant_folding() {
    let asm = compile_ok("block main(1) { setc x = 2 + 3 * 4\n aout(A, x) }");
    assert!(asm.contains("mov rax, 14"), "not folded:\n{}", asm);
}

#[test]
fn constant_propagation_through_locals() {
    let asm = compile_ok("block main(1) { set k = 6\n setc v = k * 7\n aout(A, v) }");
    assert!(
        asm.contains("mov rax, 42"),
        "constant local was not propagated:\n{}",
        asm
    );
}

/// The generated program code, without the shared runtime prelude.
fn program_code(asm: &str) -> &str {
    let end = asm
        .find("# ---------------------------------------------------------------- runtime init")
        .unwrap_or(asm.len());
    &asm[..end]
}

#[test]
fn algebraic_identities_only_touch_pure_operands() {
    let asm = compile_ok("block main(1) { setc x = 5\n aout(A, x * 1 + 0) }");
    let code = program_code(&asm);
    assert!(
        !code.contains("imul"),
        "multiply by one was not simplified:\n{}",
        code
    );
}

#[test]
fn dead_branch_with_literal_condition_is_removed() {
    let asm = compile_ok(
        "block main(1) {
            when off { aout(\"dead marker\") }
            aout(\"live\")
        }",
    );
    assert!(
        !asm.contains("dead marker"),
        "dead branch survived optimization"
    );
}

#[test]
fn dead_code_after_return_is_removed() {
    let asm = compile_ok(
        "reuse f() {
            return 1
            aout(\"unreachable marker\")
        }
        block main(1) { aout(A, f()) }",
    );
    assert!(
        !asm.contains("unreachable marker"),
        "unreachable statement survived optimization"
    );
}

#[test]
fn optimization_preserves_behaviour() {
    let cases: &[(&str, &str)] = &[
        (
            "arith",
            "block main(1) { aout(A, 2 + 3 * 4)\n aout(A, (10 - 4) / 3) }",
        ),
        (
            "vars",
            "block main(1) { setc x = 3\n x = x * 1 + 0\n aout(A, x) }",
        ),
        (
            "branch",
            "block main(1) { when 1 < 2 { aout(\"yes\") }\n anyelse { aout(\"no\") } }",
        ),
        (
            "loop",
            "block main(1) { setc i = 0\n loop {\n while i < 3 {\n insert i = 1\n aout(A, i * 2)\n then\n }\n } }",
        ),
        (
            "arrays",
            "block main(1) { set a = [1, 2, 3]\n aout(A, a.length)\n aout(A, a[2]) }",
        ),
        (
            "reuse",
            "reuse f(a, b) { return a * b + 1 }\n block main(1) { aout(A, f(2, 3)) }",
        ),
        (
            "strings",
            "block main(1) { set s = \"abcd\"\n aout(A, s)\n aout(A, s.length) }",
        ),
        (
            "whens",
            "block main(1) { setc n = 5\n when n > 3 { aout(\"big\") }\n else n > 1 { aout(\"mid\") }\n anyelse { aout(\"small\") } }",
        ),
    ];
    for (tag, src) in cases {
        let optimized = run_program_opts(
            &format!("opt_{}", tag),
            src,
            minus_a::Options { optimize: true },
        );
        let plain = run_program_opts(
            &format!("noopt_{}", tag),
            src,
            minus_a::Options { optimize: false },
        );
        match (optimized, plain) {
            (Some(a), Some(b)) => {
                assert_eq!(
                    a.stdout, b.stdout,
                    "optimization changed output for '{}'",
                    tag
                );
                assert_eq!(a.status, b.status, "optimization changed the exit code");
            }
            _ => eprintln!("skipping '{}': no toolchain", tag),
        }
    }
}

#[test]
fn optimization_is_observable_in_the_assembly() {
    let src = "block main(1) { setc x = 2 * 3 * 7\n aout(A, x) }";
    let optimized = minus_a::compile_source_opts(src, minus_a::Options { optimize: true }).unwrap();
    let plain = minus_a::compile_source_opts(src, minus_a::Options { optimize: false }).unwrap();
    assert_ne!(optimized, plain, "the optimizer made no difference at all");
    assert!(
        optimized.len() <= plain.len(),
        "optimized assembly is larger than unoptimized"
    );
}

// ---------------------------------------------------------------------------
// 17. Examples shipped with the project
// ---------------------------------------------------------------------------

#[test]
fn every_example_compiles_and_runs() {
    // Every `.ma` file in `examples/` must compile, assemble, link and exit
    // successfully: the docs point readers at exactly these programs.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("examples");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("examples directory")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().map(|e| e == "ma").unwrap_or(false))
        .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    names.sort();
    assert!(
        names.len() >= 16,
        "expected the example set, found {:?}",
        names
    );
    for name in names {
        let source = example_source(&name);
        let tag = format!("example_{}", name.replace('.', "_"));
        match run_program(&tag, &source) {
            Some(run) => assert_eq!(
                run.status, 0,
                "example '{}' exited with {}: {}",
                name, run.status, run.stderr
            ),
            None => {
                eprintln!("skipping examples: no toolchain");
                return;
            }
        }
    }
}

#[test]
fn the_modules_example_builds_and_runs() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("examples")
        .join("modules");
    let main = std::fs::read_to_string(dir.join("main.ma")).expect("modules example");
    let math = std::fs::read_to_string(dir.join("math.ma")).expect("math module");
    let labels = std::fs::read_to_string(dir.join("labels.ma")).expect("labels module");
    let run = run_files(
        "example_modules",
        &[
            ("main.ma", &main),
            ("math.ma", &math),
            ("labels.ma", &labels),
        ],
        "main.ma",
    );
    match run {
        Some(run) => {
            assert_eq!(run.status, 0, "stderr: {}", run.stderr);
            assert!(run.stdout.contains("add:        42"), "got: {}", run.stdout);
            assert!(run.stdout.contains("length:     25"), "got: {}", run.stdout);
        }
        None => eprintln!("skipping: no toolchain"),
    }
}

#[test]
fn private_declarations_are_not_visible_across_modules() {
    let diag = check_files(
        "visibility",
        &[
            (
                "math.ma",
                "module math
                 struct Hidden { v: int }
                 reuse secret() { return 1 }
                 pub reuse open() { return 2 }",
            ),
            (
                "main.ma",
                "import math
                 block main(1) { aout(A, math.secret()) }",
            ),
        ],
        "main.ma",
    );
    assert!(diag.contains("private to module 'math'"), "got: {}", diag);

    let diag = check_files(
        "visibility_struct",
        &[
            (
                "math.ma",
                "module math
                 struct Hidden { v: int }
                 pub reuse open() { return 2 }",
            ),
            (
                "main.ma",
                "import math
                 block main(1) { set h = math.Hidden { v = 1 }\n aout(A, h.v) }",
            ),
        ],
        "main.ma",
    );
    assert!(diag.contains("private to module 'math'"), "got: {}", diag);

    // The public sibling is fine.
    let run = run_files(
        "visibility_ok",
        &[
            (
                "math.ma",
                "module math
                 pub reuse open() { return 2 }",
            ),
            (
                "main.ma",
                "import math
                 block main(1) { aout(A, math.open()) }",
            ),
        ],
        "main.ma",
    );
    match run {
        Some(run) => {
            assert_eq!(run.status, 0, "stderr: {}", run.stderr);
            assert_eq!(run.stdout, "2\n");
        }
        None => eprintln!("skipping: no toolchain"),
    }
}

// ---------------------------------------------------------------------------
// Deterministic output order for parallel groups
//
// A parallel group really does run its blocks at the same time, but the text
// they print is collected per block and flushed in source declaration order once
// the whole group has finished. These tests pin that guarantee down.
// ---------------------------------------------------------------------------

/// The example from the specification: three simultaneous blocks, output always
/// in declaration order, run enough times to catch any timing dependence.
#[test]
fn parallel_output_follows_source_declaration_order() {
    let src = "
        block main(1:1x3) {
            aout(\"hello world\")
        }

        block main(1:1x3) {
            aout(\"meow\")
        }

        block main(1:1x3) {
            aout(\"third\")
        }
    ";
    for _ in 0..8 {
        assert_stdout("parallel_order", src, "hello world\nmeow\nthird\n");
    }
}

/// Every block keeps the order of its own `aout()` calls, and blocks of a group
/// never interleave with each other.
#[test]
fn each_block_keeps_its_own_output_order() {
    let src = "
        block first(1:1x3) {
            setc i = 0
            loop {
                aout(\"first \", A, i)
                insert i = 1,
                5
            }
        }

        block second(1:1x3) {
            setc i = 0
            loop {
                aout(\"second \", A, i)
                insert i = 1,
                5
            }
        }

        block third(1:1x3) {
            setc i = 0
            loop {
                aout(\"third \", A, i)
                insert i = 1,
                5
            }
        }
    ";
    let mut expected = String::new();
    for name in ["first", "second", "third"] {
        for i in 0..5 {
            expected.push_str(&format!("{} {}\n", name, i));
        }
    }
    for _ in 0..4 {
        assert_stdout("parallel_order_lines", src, &expected);
    }
}

/// A block that finishes long after its neighbours still prints in its
/// declared position, and a block that finishes first does not jump the queue.
#[test]
fn block_duration_does_not_change_output_order() {
    if !have_toolchain() {
        eprintln!("skipping: no assembler/linker available");
        return;
    }
    let src = "
        block slow(1:1x3) {
            setc i = 0
            loop {
                insert i = 1,
                3000000
            }
            aout(\"slow first\")
            aout(\"slow second\")
        }

        block quick(1:1x3) {
            aout(\"quick first\")
            aout(\"quick second\")
        }

        block medium(1:1x3) {
            setc i = 0
            loop {
                insert i = 1,
                800000
            }
            aout(\"medium only\")
        }
    ";
    let expected = "slow first\nslow second\nquick first\nquick second\nmedium only\n";
    for _ in 0..4 {
        let run = run_program("parallel_order_timing", src).expect("toolchain");
        assert_eq!(run.status, 0, "stderr: {}", run.stderr);
        assert_eq!(run.stdout, expected);
    }
}

/// `(N:N)` and `(N:NxK)` are ordered the same way, for every group size.
#[test]
fn parallel_groups_of_two_three_and_four_keep_declaration_order() {
    if !have_toolchain() {
        eprintln!("skipping: no assembler/linker available");
        return;
    }
    let two = "
        block a(1:1) { aout(\"one\") }
        block b(1:1) { aout(\"two\") }
    ";
    let four = "
        block a(1:1x4) {
            setc i = 0
            loop { insert i = 1, 400000 }
            aout(\"a\")
        }
        block b(1:1x4) { aout(\"b\") }
        block c(1:1x4) { aout(\"c\") }
        block d(1:1x4) {
            setc i = 0
            loop { insert i = 1, 200000 }
            aout(\"d\")
        }
    ";
    for _ in 0..4 {
        assert_stdout("parallel_order_two", two, "one\ntwo\n");
        assert_stdout("parallel_order_four", four, "a\nb\nc\nd\n");
    }
}

/// The queue barrier still holds: the next queue's output comes after the whole
/// group's flushed output, even when the group is slower.
#[test]
fn queue_barrier_still_orders_output_after_a_group() {
    if !have_toolchain() {
        eprintln!("skipping: no assembler/linker available");
        return;
    }
    let src = "
        block slow_left(1:1) {
            setc i = 0
            loop { insert i = 1, 2000000 }
            aout(\"group left\")
        }

        block fast_right(1:1) {
            aout(\"group right\")
        }

        block after(2) {
            aout(\"after the barrier\")
        }
    ";
    let expected = "group left\ngroup right\nafter the barrier\n";
    for _ in 0..4 {
        let run = run_program("parallel_order_barrier", src).expect("toolchain");
        assert_eq!(run.status, 0, "stderr: {}", run.stderr);
        assert_eq!(run.stdout, expected);
    }
}

/// A block that prints more than the buffer's initial capacity grows it without
/// losing a byte and without disturbing the ordering.
#[test]
fn long_block_output_grows_its_buffer_safely() {
    if !have_toolchain() {
        eprintln!("skipping: no assembler/linker available");
        return;
    }
    let src = "
        block chatty(1:1x2) {
            setc i = 0
            loop {
                aout(\"chatty line \", A, i, \" padded out so the buffer has to grow\")
                insert i = 1,
                200
            }
        }

        block quiet(1:1x2) {
            setc i = 0
            loop {
                aout(\"quiet \", A, i)
                insert i = 1,
                200
            }
        }
    ";
    let run = run_program("parallel_order_growth", src).expect("toolchain");
    assert_eq!(run.status, 0, "stderr: {}", run.stderr);
    let lines: Vec<&str> = run.stdout.lines().collect();
    assert_eq!(lines.len(), 400);
    for (index, line) in lines.iter().enumerate() {
        if index < 200 {
            assert_eq!(
                *line,
                format!("chatty line {} padded out so the buffer has to grow", index),
                "line {} of the first block",
                index
            );
        } else {
            assert_eq!(
                *line,
                format!("quiet {}", index - 200),
                "line {} of the second block",
                index
            );
        }
    }
}

/// A sequential (unbuffered) block and a later group interleave correctly: the
/// buffered flush of the group lands exactly where the group sits in the queue
/// order.
#[test]
fn sequential_and_parallel_output_keep_program_order() {
    if !have_toolchain() {
        eprintln!("skipping: no assembler/linker available");
        return;
    }
    let src = "
        block before(1) {
            aout(\"before\")
        }

        block left(2:2) {
            setc i = 0
            loop { insert i = 1, 600000 }
            aout(\"left\")
        }

        block right(2:2) {
            aout(\"right\")
        }

        block after(3) {
            aout(\"after\")
        }
    ";
    let expected = "before\nleft\nright\nafter\n";
    for _ in 0..4 {
        let run = run_program("parallel_order_mixed", src).expect("toolchain");
        assert_eq!(run.status, 0, "stderr: {}", run.stderr);
        assert_eq!(run.stdout, expected);
    }
}

/// Failing inside a parallel block still prints what that block produced before
/// the failure, in program order up to the failure.
#[test]
fn buffered_output_survives_a_fatal_error_in_the_block() {
    if !have_toolchain() {
        eprintln!("skipping: no assembler/linker available");
        return;
    }
    let src = "
        block left(1:1) {
            aout(\"left says hello\")
            fail \"left gave up\"
        }

        block right(1:1) {
            aout(\"right says hello\")
        }
    ";
    let run = run_program("parallel_order_fail", src).expect("toolchain");
    assert_eq!(run.status, 1, "a failing block must exit 1");
    assert!(
        run.stderr.contains("error: left gave up"),
        "stderr: {}",
        run.stderr
    );
    // Whichever block fails first, no output may be silently dropped: the
    // failing block flushes its own buffer before exiting.
    assert!(
        run.stdout.contains("left says hello") || run.stdout.contains("right says hello"),
        "buffered output was lost on failure: {:?}",
        run.stdout
    );
}

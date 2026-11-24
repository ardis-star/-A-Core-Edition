//! Invalid programs: every Core feature must be rejected with a specific,
//! located diagnostic when it is misused.
//!
//! Each test compiles a small program that must fail, and checks that the
//! diagnostic names the problem (and, where it matters, the location) instead
//! of failing with a generic message.

mod support;

use support::*;

fn rejects(tag: &str, source: &str, expected: &str) {
    let err = compile_err(source);
    assert!(
        err.contains(expected),
        "'{}' produced an unexpected diagnostic (wanted {:?}):\n{}",
        tag,
        expected,
        err
    );
}

// --- names and declarations ------------------------------------------------

#[test]
fn undefined_variable_is_rejected() {
    rejects(
        "undefined_var",
        "block main(1) { aout(A, nope) }",
        "undefined variable 'nope'",
    );
}

#[test]
fn variable_names_must_be_snake_case() {
    rejects(
        "snake_case",
        "block main(1) { set Foo = 1 }",
        "must be snake_case",
    );
}

#[test]
fn duplicate_local_is_rejected() {
    rejects(
        "duplicate_local",
        "block main(1) { set x = 1\n set x = 2 }",
        "already declared",
    );
}

#[test]
fn assigning_a_fixed_variable_is_rejected() {
    rejects(
        "fixed_assign",
        "block main(1) { set x = 1\n x = 2 }",
        "declared with 'set' (fixed)",
    );
}

#[test]
fn changing_a_variables_type_is_rejected() {
    rejects(
        "type_change",
        "block main(1) { setc x = 1\n x = \"s\" }",
        "types are fixed at declaration",
    );
}

#[test]
fn insert_on_a_fixed_variable_is_rejected() {
    rejects(
        "insert_fixed",
        "block main(1) { set x = 1\n insert x = 5 }",
        "cannot modify 'x'",
    );
}

// --- expressions -----------------------------------------------------------

#[test]
fn mixed_type_arithmetic_is_rejected() {
    rejects(
        "mixed_arith",
        "block main(1) { set x = 1 + \"s\" }",
        "requires numeric operands",
    );
}

#[test]
fn non_boolean_condition_is_rejected() {
    rejects(
        "bad_cond",
        "block main(1) { when 5 { aout(\"x\") } }",
        "must be bool",
    );
}

#[test]
fn struct_equality_is_rejected() {
    rejects(
        "struct_eq",
        "struct P { x: int }\n block main(1) { set p = P { x = 1 }\n when p == p { aout(\"x\") } }",
        "cannot compare",
    );
}

#[test]
fn writing_a_wrong_type_through_a_pointer_is_rejected() {
    rejects(
        "pointer_type",
        "block main(1) { setc x = 1\n set p = &x\n *p = \"s\" }",
        "cannot assign string to int",
    );
}

#[test]
fn or_is_rejected_with_guidance() {
    rejects(
        "or_keyword",
        "block main(1) { when on or off { aout(\"x\") } }",
        "'or' is not part of -A Core",
    );
}

#[test]
fn semicolon_is_a_lex_error() {
    rejects(
        "semicolon",
        "block main(1) { set x = 1; aout(A, x) }",
        "E_LEX",
    );
}

#[test]
fn unterminated_string_is_a_lex_error() {
    rejects(
        "unterminated",
        "block main(1) { aout(\"abc) }",
        "unterminated string",
    );
}

#[test]
fn empty_program_is_rejected() {
    rejects("empty", "", "E_");
}

// --- blocks and queues -----------------------------------------------------

#[test]
fn queue_zero_is_rejected() {
    rejects(
        "queue_zero",
        "block a(0) { aout(\"x\") }",
        "queues start at 1",
    );
}

#[test]
fn duplicate_queue_assignment_is_rejected() {
    rejects(
        "queue_dup",
        "block a(1) { aout(\"x\") }\n block b(1) { aout(\"y\") }",
        "duplicate assignment to queue 1",
    );
}

#[test]
fn mismatched_group_numbers_are_rejected() {
    rejects(
        "group_numbers",
        "block a(1:2) { aout(\"x\") }",
        "both numbers must match",
    );
}

#[test]
fn group_with_too_few_blocks_is_rejected() {
    rejects(
        "group_missing",
        "block a(1:1x3) { aout(\"x\") }\n block b(1:1x3) { aout(\"y\") }",
        "missing parallel group members",
    );
}

#[test]
fn group_with_too_many_blocks_is_rejected() {
    rejects(
        "group_excess",
        "block a(1:1) { aout(\"x\") }\n block b(1:1) { aout(\"y\") }\n block c(1:1) { aout(\"z\") }",
        "excess parallel group members",
    );
}

#[test]
fn mixed_group_and_solo_blocks_are_rejected() {
    rejects(
        "group_mixed",
        "block a(1:1) { aout(\"x\") }\n block b(1) { aout(\"y\") }",
        "inconsistent parallel group",
    );
}

#[test]
fn block_name_must_be_snake_case() {
    rejects(
        "block_case",
        "block Main(1) { aout(\"x\") }",
        "must use snake_case",
    );
}

#[test]
fn aout_requires_the_marker_inside_the_call() {
    rejects(
        "aout_marker",
        "block main(1) { aout(x) }",
        "aout() arguments must be string literals or 'A'",
    );
}

// --- reuse -----------------------------------------------------------------

#[test]
fn attaching_an_unknown_reuse_is_rejected() {
    rejects(
        "unknown_reuse",
        "block main(nope, 1) { aout(A, nope) }",
        "undefined reuse 'nope'",
    );
}

#[test]
fn reuse_arity_mismatch_is_rejected() {
    rejects(
        "reuse_arity",
        "reuse f(a, b) { return a }\n block main(1) { aout(A, f(1)) }",
        "expects 2 argument(s)",
    );
}

#[test]
fn calling_an_unknown_function_is_rejected() {
    rejects(
        "unknown_fn",
        "block main(1) { aout(A, nope(1)) }",
        "unknown function 'nope'",
    );
}

#[test]
fn attaching_a_reuse_that_returns_nothing_is_rejected() {
    rejects(
        "void_reuse",
        "reuse nothing() { aout(\"x\") }\n block main(nothing, 1) { aout(\"y\") }",
        "returns nothing",
    );
}

#[test]
fn duplicate_reuse_definition_is_rejected() {
    rejects(
        "duplicate_reuse",
        "reuse f() { return 1 }\n reuse f() { return 2 }\n block main(1) { aout(A, f()) }",
        "duplicate reuse",
    );
}

#[test]
fn return_outside_a_reuse_is_rejected() {
    rejects(
        "return_block",
        "block main(1) { return }",
        "only valid inside a reuse",
    );
}

// --- structs ---------------------------------------------------------------

#[test]
fn unknown_struct_field_is_rejected() {
    rejects(
        "struct_field",
        "struct P { x: int }\n block main(1) { set p = P { x = 1 }\n aout(A, p.y) }",
        "has no field 'y'",
    );
}

#[test]
fn missing_struct_field_is_rejected() {
    rejects(
        "struct_missing",
        "struct P { x: int, y: int }\n block main(1) { set p = P { x = 1 } }",
        "missing field 'y'",
    );
}

#[test]
fn extra_struct_field_is_rejected() {
    rejects(
        "struct_extra",
        "struct P { x: int }\n block main(1) { set p = P { x = 1, z = 2 } }",
        "has no field 'z'",
    );
}

#[test]
fn struct_literal_with_wrong_field_type_is_rejected() {
    rejects(
        "struct_field_type",
        "struct P { x: int }\n block main(1) { set p = P { x = \"s\" } }",
        "E_TYPE",
    );
}

#[test]
fn duplicate_struct_is_rejected() {
    rejects(
        "struct_dup",
        "struct P { x: int }\n struct P { y: int }",
        "duplicate struct",
    );
}

// --- enums -----------------------------------------------------------------

#[test]
fn unknown_enum_variant_is_rejected() {
    rejects(
        "enum_variant",
        "enum Color { red }\n block main(1) { aout(A, Color.blue) }",
        "has no variant 'blue'",
    );
}

#[test]
fn duplicate_enum_variant_is_rejected() {
    rejects(
        "enum_dup_variant",
        "enum Color { red, red }",
        "duplicate variant",
    );
}

// --- traits and impls ------------------------------------------------------

#[test]
fn impl_of_unknown_trait_is_rejected() {
    rejects(
        "impl_trait",
        "struct S { v: int }\n impl Nope for S { }",
        "unknown trait",
    );
}

#[test]
fn impl_for_unknown_type_is_rejected() {
    rejects(
        "impl_type",
        "trait T { reuse f(self) -> int }\n impl T for Missing { }",
        "unknown type",
    );
}

#[test]
fn trait_method_signature_mismatch_is_rejected() {
    rejects(
        "trait_signature",
        "struct S { v: int }
         trait T { reuse f(self) -> int }
         impl T for S { reuse f(self) -> string { return \"x\" } }",
        "E_TYPE",
    );
}

#[test]
fn calling_a_missing_method_is_rejected() {
    rejects(
        "missing_method",
        "struct S { v: int }\n block main(1) { set s = S { v = 1 }\n aout(A, s.f()) }",
        "no method 'f'",
    );
}

// --- intrinsic and module misuse -------------------------------------------

#[test]
fn unknown_intrinsic_is_rejected() {
    rejects(
        "intrinsic",
        "block main(1) { aout(A, @nope()) }",
        "unknown intrinsic '@nope'",
    );
}

#[test]
fn intrinsic_with_wrong_argument_type_is_rejected() {
    rejects(
        "intrinsic_type",
        "block main(1) { set p = @alloc(\"s\") }",
        "@alloc",
    );
}

#[test]
fn foundation_namespace_is_rejected() {
    rejects(
        "foundation",
        "block main(1) { aout(A, make.shape.circle) }",
        "Foundation",
    );
}

#[test]
fn importing_a_missing_file_is_reported() {
    let diag = check_files(
        "import_missing",
        &[("main.ma", "import nowhere\nblock main(1) { aout(\"x\") }")],
        "main.ma",
    );
    assert!(diag.contains("cannot read module"), "got: {}", diag);
}

#[test]
fn import_cycles_are_reported() {
    let diag = check_files(
        "import_cycle",
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

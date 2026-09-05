#[test]
fn compile_fail_ownership_cases() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/*.rs");
}

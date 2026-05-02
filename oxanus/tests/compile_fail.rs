#[test]
fn batch_worker_requires_process_batch_hook() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/batch_missing_process_batch.rs");
    t.compile_fail("tests/ui/batch_size_requires_timeout.rs");
    t.compile_fail("tests/ui/batch_size_zero.rs");
    t.compile_fail("tests/ui/batch_timeout_requires_size.rs");
}

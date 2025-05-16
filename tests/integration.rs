mod integration {
    use test_each_file::test_each_file;

    async fn run_async_test([content]: [&str; 1]) {
        assert!(content.len() > 0)
    }

    test_each_file! {#[tokio::test] async for ["psf"] in "tests/fixtures" => run_async_test}
}

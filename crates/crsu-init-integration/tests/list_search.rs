use crsu::init_test_support::{OverflowScreen, render_search_results};

#[test]
fn slash_search_keeps_non_matches_visible_and_shows_the_active_query() {
    let frame = render_search_results(OverflowScreen::Project, "11", 50, 100, 16);

    assert!(frame.contains("PROJECT-11"), "match is missing:\n{frame}");
    assert!(
        frame.contains("PROJECT-10"),
        "non-match is hidden:\n{frame}"
    );
    assert!(frame.contains("/11"), "query is missing:\n{frame}");
}

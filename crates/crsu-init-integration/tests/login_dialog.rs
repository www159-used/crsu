use crsu::init_test_support::render_login_dialog;

#[test]
fn authentication_submission_renders_a_signing_in_dialog_before_the_request_runs() {
    let frame = render_login_dialog(100, 24);

    assert!(
        frame.contains("Signing in and loading candidates..."),
        "loading message is missing:\n{frame}"
    );
    assert!(frame.contains("Loading"), "dialog is missing:\n{frame}");
    assert!(
        frame.contains("-- NORMAL --"),
        "mode is missing from the footer:\n{frame}"
    );
}

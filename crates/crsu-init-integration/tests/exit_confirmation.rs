use crsu::init_test_support::render_exit_confirmation;

#[test]
fn escape_requires_confirmation_before_discarding_init_setup() {
    let frame = render_exit_confirmation(100, 24);

    assert!(
        frame.contains("Discard setup?"),
        "confirmation is missing:\n{frame}"
    );
    assert!(
        frame.contains("Enter/y: discard"),
        "confirm action is missing:\n{frame}"
    );
    assert!(
        frame.contains("Esc/n: continue"),
        "cancel action is missing:\n{frame}"
    );
}

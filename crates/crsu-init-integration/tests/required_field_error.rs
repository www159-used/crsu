use crsu::init_test_support::render_required_field_error;

#[test]
fn missing_required_value_keeps_the_form_open_and_explains_how_to_continue() {
    let frame = render_required_field_error(100, 24);

    assert!(
        frame.contains("Required field"),
        "error title is missing:\n{frame}"
    );
    assert!(
        frame.contains("Crucible URL is required"),
        "error is missing:\n{frame}"
    );
    assert!(
        frame.contains("Enter/Esc: continue editing"),
        "recovery action is missing:\n{frame}"
    );
}

use crsu::init_test_support::{FormFlow, InputMode, Step};

#[test]
fn text_steps_use_vim_normal_and_insert_modes_and_list_steps_can_go_back() {
    let mut form = FormFlow::new();
    assert_eq!(form.input_mode(), InputMode::Normal);

    form.enter_insert();
    assert_eq!(form.input_mode(), InputMode::Insert);
    form.exit_insert();
    assert_eq!(form.input_mode(), InputMode::Normal);

    form.advance();
    form.advance();
    assert_eq!(form.step(), Step::Project);
    form.back();
    assert_eq!(form.step(), Step::Authentication);
}

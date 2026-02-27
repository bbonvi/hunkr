use super::shell_command::ShellOutputPolicy;

#[test]
fn shell_output_policy_normalizes_and_sanitizes_chunks() {
    let policy = ShellOutputPolicy::new(4, 8, 8, 64);
    let mut lines = Vec::new();
    let mut tail = String::new();

    let overflow =
        policy.append_sanitized_chunk(&mut lines, &mut tail, "ok\r\n\u{1b}[31mred\u{1b}[0m\rnext");

    assert_eq!(overflow, 0);
    assert_eq!(lines, vec!["ok".to_owned(), "red".to_owned()]);
    assert_eq!(tail, "next");
}

#[test]
fn shell_output_policy_trims_overflowing_lines_and_partial_tail() {
    let policy = ShellOutputPolicy::new(4, 8, 2, 4);
    let mut lines = vec!["old0".to_owned(), "old1".to_owned()];
    let mut tail = String::new();

    let overflow = policy.append_sanitized_chunk(&mut lines, &mut tail, "new0\nnew1\nwxyz123");

    assert_eq!(overflow, 2);
    assert_eq!(lines, vec!["new0".to_owned(), "new1".to_owned()]);
    assert_eq!(tail, "z123");
}

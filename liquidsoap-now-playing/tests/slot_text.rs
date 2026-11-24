//! Checks of the slot text `liquidsoap/musicindex-state.liq`. No Liquidsoap is
//! on the build host, so these checks find known parse faults in the text.

const SLOT_TEXT: &str = include_str!("../../liquidsoap/musicindex-state.liq");

/// The body of an anonymous function is one expression. A body on the next
/// lines must be one `if` or `try` block, or it must start with `begin`.
/// Bundle 9203797 failed on the station with "Parse error" because a body had
/// a binding and an `if` with no `begin` (ADR 0028, task 012).
#[test]
fn multi_line_anonymous_function_body_is_one_block() {
    let lines: Vec<&str> = SLOT_TEXT.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let code = line.split('#').next().unwrap_or("").trim_end();
        if !(code.contains("fun ") && code.ends_with("->")) {
            continue;
        }
        let body = lines[index + 1..]
            .iter()
            .map(|next| next.trim())
            .find(|next| !next.is_empty() && !next.starts_with('#'))
            .unwrap_or("");
        assert!(
            body.starts_with("if ") || body == "try",
            "liquidsoap/musicindex-state.liq line {}: the body of an anonymous \
             function is one expression. Write `-> begin` and close the body \
             with `end` (ADR 0028, task 012).",
            index + 1
        );
    }
}

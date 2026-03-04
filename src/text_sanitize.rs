/// Strip ANSI/control sequences that can mutate terminal state when rendered in the TUI.
///
/// Keeps normal printable text plus newlines/tabs, while removing escape-driven control flows
/// like CSI/OSC and other control bytes.
pub(crate) fn sanitize_terminal_text(input: &str) -> String {
    #[derive(Clone, Copy)]
    enum SanitizeState {
        Normal,
        Escape,
        Csi,
        Osc,
        OscEscape,
        St,
        StEscape,
    }

    let mut out = String::with_capacity(input.len());
    let mut state = SanitizeState::Normal;

    for ch in input.chars() {
        state = match state {
            SanitizeState::Normal => match ch {
                '\u{1b}' => SanitizeState::Escape,
                '\u{9b}' => SanitizeState::Csi,
                '\u{9d}' => SanitizeState::Osc,
                '\u{90}' | '\u{98}' | '\u{9e}' | '\u{9f}' => SanitizeState::St,
                '\n' | '\t' => {
                    out.push(ch);
                    SanitizeState::Normal
                }
                _ if !ch.is_control() => {
                    out.push(ch);
                    SanitizeState::Normal
                }
                _ => SanitizeState::Normal,
            },
            SanitizeState::Escape => match ch {
                '[' => SanitizeState::Csi,
                ']' => SanitizeState::Osc,
                'P' | 'X' | '^' | '_' => SanitizeState::St,
                _ => SanitizeState::Normal,
            },
            SanitizeState::Csi => {
                if ('@'..='~').contains(&ch) {
                    SanitizeState::Normal
                } else {
                    SanitizeState::Csi
                }
            }
            SanitizeState::Osc => match ch {
                '\u{7}' => SanitizeState::Normal,
                '\u{1b}' => SanitizeState::OscEscape,
                _ => SanitizeState::Osc,
            },
            SanitizeState::OscEscape => match ch {
                '\\' => SanitizeState::Normal,
                '\u{1b}' => SanitizeState::OscEscape,
                _ => SanitizeState::Osc,
            },
            SanitizeState::St => match ch {
                '\u{1b}' => SanitizeState::StEscape,
                _ => SanitizeState::St,
            },
            SanitizeState::StEscape => match ch {
                '\\' => SanitizeState::Normal,
                '\u{1b}' => SanitizeState::StEscape,
                _ => SanitizeState::St,
            },
        };
    }

    out
}

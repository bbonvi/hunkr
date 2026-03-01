use super::super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum PaneCycleDirection {
    Prev,
    Next,
}

pub(in crate::app) fn theme_toggle_conflicts_with_diff_pending_op(
    key: KeyEvent,
    focused: FocusPane,
    pending_op: Option<DiffPendingOp>,
) -> bool {
    key.modifiers == KeyModifiers::NONE
        && key.code == KeyCode::Char('t')
        && focused == FocusPane::Diff
        && matches!(pending_op, Some(DiffPendingOp::Z))
}

/// Maps pane-cycle keyboard shortcuts while preserving Ctrl-modified bindings.
pub(in crate::app) fn pane_focus_cycle_direction(key: KeyEvent) -> Option<PaneCycleDirection> {
    match (key.code, key.modifiers) {
        (KeyCode::Tab, KeyModifiers::NONE) => Some(PaneCycleDirection::Next),
        (KeyCode::Tab, KeyModifiers::SHIFT) => Some(PaneCycleDirection::Prev),
        (KeyCode::BackTab, KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            Some(PaneCycleDirection::Prev)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_focus_cycle_direction_accepts_tab_and_backtab_variants() {
        assert_eq!(
            pane_focus_cycle_direction(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            Some(PaneCycleDirection::Next)
        );
        assert_eq!(
            pane_focus_cycle_direction(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT)),
            Some(PaneCycleDirection::Prev)
        );
        assert_eq!(
            pane_focus_cycle_direction(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE)),
            Some(PaneCycleDirection::Prev)
        );
        assert_eq!(
            pane_focus_cycle_direction(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)),
            Some(PaneCycleDirection::Prev)
        );
    }

    #[test]
    fn theme_toggle_conflict_requires_diff_focus_and_pending_z_op() {
        let key = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE);
        assert!(theme_toggle_conflicts_with_diff_pending_op(
            key,
            FocusPane::Diff,
            Some(DiffPendingOp::Z)
        ));
        assert!(!theme_toggle_conflicts_with_diff_pending_op(
            key,
            FocusPane::Files,
            Some(DiffPendingOp::Z)
        ));
        assert!(!theme_toggle_conflicts_with_diff_pending_op(
            key,
            FocusPane::Diff,
            None
        ));
    }
}

use super::super::App;

/// Base behavior for pane-specific view-model builders.
pub(in crate::app) trait PaneViewModelBuilder {
    type Output;

    fn build(&self, app: &App) -> Self::Output;
}

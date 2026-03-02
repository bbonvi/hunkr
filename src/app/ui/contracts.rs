/// Base behavior for pane-specific view-model builders.
pub(in crate::app) trait PaneViewModelBuilder<Input> {
    type Output;

    fn build(&self, input: &Input) -> Self::Output;
}

/// Base behavior for pane-specific view-model builders.
pub(in crate::app) trait PaneViewModelBuilder<Input> {
    type Output<'a>
    where
        Input: 'a;

    fn build<'a>(&self, input: &'a Input) -> Self::Output<'a>;
}

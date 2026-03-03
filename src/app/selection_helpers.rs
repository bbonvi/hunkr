//! Commit selection and status-transition operations.
use crate::app::*;

pub(super) fn commit_mouse_selection_mode(modifiers: KeyModifiers) -> CommitMouseSelectionMode {
    if modifiers.contains(KeyModifiers::SHIFT) {
        return CommitMouseSelectionMode::Range;
    }
    CommitMouseSelectionMode::Replace
}

/// Resolves the fixed anchor used when extending commit ranges from keyboard `Space`.
pub(super) fn range_anchor_for_space(
    rows: &[CommitRow],
    selection_anchor: Option<usize>,
    cursor: usize,
) -> usize {
    if let Some(anchor) = selection_anchor.filter(|idx| *idx < rows.len()) {
        return anchor;
    }

    let has_selection = rows.iter().any(|row| row.selected);
    if !has_selection {
        return cursor;
    }

    rows.iter()
        .enumerate()
        .filter(|(_, row)| row.selected)
        .min_by_key(|(idx, _)| idx.abs_diff(cursor))
        .map(|(idx, _)| idx)
        .unwrap_or(cursor)
}

pub(super) fn selected_ids_oldest_first(rows: &[CommitRow]) -> Vec<String> {
    rows.iter()
        .rev()
        .filter(|row| row.selected && !row.is_uncommitted)
        .map(|row| row.info.id.clone())
        .collect()
}

pub(super) fn index_of_commit(rows: &[CommitRow], commit_id: &str) -> Option<usize> {
    rows.iter().position(|row| row.info.id == commit_id)
}

#[cfg(test)]
pub(super) fn restore_list_index_by_commit_id(
    rows: &[CommitRow],
    previous_commit_id: Option<&str>,
    fallback_index: Option<usize>,
) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    if let Some(commit_id) = previous_commit_id
        && let Some(idx) = index_of_commit(rows, commit_id)
    {
        return Some(idx);
    }
    fallback_index
        .map(|idx| idx.min(rows.len() - 1))
        .or(Some(0))
}

pub(super) fn merge_aggregate_diff(base: &mut AggregatedDiff, next: AggregatedDiff) {
    for (path, change) in next.file_changes {
        base.file_changes
            .entry(path)
            .and_modify(|current| merge_file_change_summary(current, &change))
            .or_insert(change);
    }
    for (path, mut patch) in next.files {
        base.files
            .entry(path.clone())
            .or_insert_with(|| FilePatch {
                path,
                hunks: Vec::new(),
            })
            .hunks
            .append(&mut patch.hunks);
    }
}

fn merge_file_change_summary(current: &mut FileChangeSummary, next: &FileChangeSummary) {
    current.additions = current.additions.saturating_add(next.additions);
    current.deletions = current.deletions.saturating_add(next.deletions);
    if current.old_path.is_none() {
        current.old_path = next.old_path.clone();
    }
    current.kind = merged_change_kind(current.kind, next.kind);
}

fn merged_change_kind(current: FileChangeKind, next: FileChangeKind) -> FileChangeKind {
    use FileChangeKind::*;
    match (current, next) {
        (_, Unknown) => current,
        (Unknown, _) => next,
        (Added, Deleted) | (Deleted, Added) => Modified,
        // Keep richer upstream classification when the next layer only reports "modified".
        (Renamed | Copied | TypeChanged, Modified) => current,
        (_, next) => next,
    }
}

pub(super) fn apply_range_selection(rows: &mut [CommitRow], start: usize, end: usize) {
    let (start, end) = (min(start, end), max(start, end));
    for (idx, row) in rows.iter_mut().enumerate() {
        row.selected = idx >= start && idx <= end;
    }
}

pub(super) fn select_only_index(rows: &mut [CommitRow], selected_idx: usize) {
    for (idx, row) in rows.iter_mut().enumerate() {
        row.selected = idx == selected_idx;
    }
}

pub(super) fn apply_status_ids(
    rows: &mut [CommitRow],
    ids: &BTreeSet<String>,
    status: ReviewStatus,
) {
    for row in rows {
        if ids.contains(&row.info.id) {
            row.status = status;
        }
    }
}

pub(super) fn apply_status_transition(
    rows: &mut [CommitRow],
    ids: &BTreeSet<String>,
    status: ReviewStatus,
) {
    apply_status_ids(rows, ids, status);
}

pub(super) fn deselect_rows_outside_status_filter(
    rows: &mut [CommitRow],
    status_filter: CommitStatusFilter,
) -> usize {
    let mut deselected = 0usize;
    for row in rows {
        if row.selected && !status_filter.matches_row(row) {
            row.selected = false;
            deselected += 1;
        }
    }
    deselected
}

pub(super) fn selected_rows_hidden_by_status_filter(
    rows: &[CommitRow],
    status_filter: CommitStatusFilter,
) -> usize {
    rows.iter()
        .filter(|row| row.selected && !status_filter.matches_row(row))
        .count()
}

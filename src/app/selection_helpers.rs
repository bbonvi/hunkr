//! Commit selection and status-transition operations.
use super::*;

pub(super) fn commit_mouse_selection_mode(modifiers: KeyModifiers) -> CommitMouseSelectionMode {
    if modifiers.contains(KeyModifiers::SHIFT) {
        return CommitMouseSelectionMode::Range;
    }
    if modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) {
        return CommitMouseSelectionMode::Toggle;
    }
    CommitMouseSelectionMode::Replace
}

pub(super) fn apply_toggle_range_from_baseline(
    rows: &mut [CommitRow],
    baseline: &[bool],
    start: usize,
    end: usize,
) {
    if rows.len() != baseline.len() {
        return;
    }
    let (start, end) = (min(start, end), max(start, end));
    for (idx, row) in rows.iter_mut().enumerate() {
        row.selected = if idx >= start && idx <= end {
            !baseline[idx]
        } else {
            baseline[idx]
        };
    }
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

pub(super) fn auto_deselect_status(status: ReviewStatus) -> bool {
    matches!(status, ReviewStatus::Reviewed | ReviewStatus::Resolved)
}

pub(super) fn deselect_ids(rows: &mut [CommitRow], ids: &BTreeSet<String>) {
    for row in rows {
        if ids.contains(&row.info.id) {
            row.selected = false;
        }
    }
}

pub(super) fn apply_status_transition(
    rows: &mut [CommitRow],
    ids: &BTreeSet<String>,
    status: ReviewStatus,
) {
    apply_status_ids(rows, ids, status);
    if auto_deselect_status(status) {
        deselect_ids(rows, ids);
    }
}

pub(super) fn selected_ids_will_change_for_status_update(
    rows: &[CommitRow],
    ids: &BTreeSet<String>,
    status: ReviewStatus,
) -> bool {
    if !auto_deselect_status(status) {
        return false;
    }
    rows.iter()
        .any(|row| row.selected && ids.contains(&row.info.id))
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

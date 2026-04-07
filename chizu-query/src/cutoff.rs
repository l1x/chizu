use chizu_core::CutoffMode;

/// Apply score-gap cutoff to a descending-sorted score list.
/// Returns the number of results to keep.
pub fn apply_cutoff(
    scores: &[f64],
    mode: &CutoffMode,
    threshold: f64,
    min_results: usize,
    max_results: usize,
) -> usize {
    match mode {
        CutoffMode::None => scores.len(),
        CutoffMode::RelativeGap => relative_gap(scores, threshold, min_results, max_results),
    }
}

fn relative_gap(scores: &[f64], threshold: f64, min_results: usize, max_results: usize) -> usize {
    let n = scores.len();
    if n <= min_results {
        return n;
    }

    let effective_max = max_results.min(n);

    for (i, &score) in scores.iter().enumerate().take(min_results.min(n)) {
        tracing::debug!("rank={} score={:.2}", i + 1, score);
    }

    // Check gap starting at min_results
    for i in min_results..effective_max {
        let prev = scores[i - 1];
        let curr = scores[i];
        let ratio = if prev > 0.0 { curr / prev } else { 0.0 };

        if ratio < threshold {
            tracing::debug!(
                "rank={} score={:.2}  <-- cutoff triggered (ratio={:.2} < {:.2})",
                i + 1,
                curr,
                ratio,
                threshold
            );
            return i;
        }
        tracing::debug!("rank={} score={:.2}", i + 1, curr);
    }

    effective_max
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cutoff_none_returns_all() {
        let scores = vec![1.0, 0.9, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3];
        assert_eq!(apply_cutoff(&scores, &CutoffMode::None, 0.80, 3, 8), 8);
    }

    #[test]
    fn test_cutoff_relative_gap_triggers() {
        // Sharp drop after position 4 (index 3→4): 0.50/0.70 = 0.71 < 0.80
        let scores = vec![1.0, 0.90, 0.80, 0.70, 0.50, 0.40, 0.30];
        let keep = apply_cutoff(&scores, &CutoffMode::RelativeGap, 0.80, 3, 8);
        assert_eq!(keep, 4);
    }

    #[test]
    fn test_cutoff_respects_min_results() {
        // Sharp drop at position 2 (index 1→2), but min_results=3
        let scores = vec![1.0, 0.90, 0.10, 0.05];
        let keep = apply_cutoff(&scores, &CutoffMode::RelativeGap, 0.80, 3, 8);
        // min_results=3, gap check starts at index 3. scores[3]/scores[2] = 0.05/0.10 = 0.50 < 0.80
        assert_eq!(keep, 3);
    }

    #[test]
    fn test_cutoff_respects_max_results() {
        // No gap triggers, so max_results caps output
        let scores = vec![1.0, 0.95, 0.90, 0.86, 0.82, 0.78, 0.75, 0.72, 0.70, 0.68];
        let keep = apply_cutoff(&scores, &CutoffMode::RelativeGap, 0.80, 3, 5);
        assert_eq!(keep, 5);
    }

    #[test]
    fn test_cutoff_fewer_than_min() {
        let scores = vec![1.0, 0.5];
        let keep = apply_cutoff(&scores, &CutoffMode::RelativeGap, 0.80, 3, 8);
        assert_eq!(keep, 2); // Only 2 results, less than min_results
    }

    #[test]
    fn test_cutoff_single_result() {
        let scores = vec![0.9];
        let keep = apply_cutoff(&scores, &CutoffMode::RelativeGap, 0.80, 3, 8);
        assert_eq!(keep, 1);
    }

    #[test]
    fn test_cutoff_empty() {
        let scores: Vec<f64> = vec![];
        let keep = apply_cutoff(&scores, &CutoffMode::RelativeGap, 0.80, 3, 8);
        assert_eq!(keep, 0);
    }

    #[test]
    fn test_cutoff_all_equal_scores() {
        let scores = vec![0.5, 0.5, 0.5, 0.5, 0.5];
        let keep = apply_cutoff(&scores, &CutoffMode::RelativeGap, 0.80, 3, 8);
        assert_eq!(keep, 5); // ratio=1.0 everywhere, no cutoff
    }

    #[test]
    fn test_cutoff_zero_scores() {
        let scores = vec![0.5, 0.3, 0.0, 0.0];
        let keep = apply_cutoff(&scores, &CutoffMode::RelativeGap, 0.80, 3, 8);
        // At index 3: scores[2]=0.0, ratio = 0/0.0 → 0.0 < 0.80 → cut
        assert_eq!(keep, 3);
    }

    #[test]
    fn test_cutoff_gradual_decline_no_trigger() {
        // Each step is ~95% of previous — no cutoff triggers
        let scores = vec![1.0, 0.95, 0.90, 0.855, 0.812, 0.771];
        let keep = apply_cutoff(&scores, &CutoffMode::RelativeGap, 0.80, 3, 8);
        assert_eq!(keep, 6); // All pass threshold
    }
}

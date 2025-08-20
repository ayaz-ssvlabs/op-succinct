use sp1_sdk::{CudaProver, ProverClient};
use std::sync::Arc;
use anyhow::Result;

/// Creates a CUDA ProverClient for validity proposer.
/// 
/// # Returns
/// 
/// * `Result<Arc<CudaProver>>` - The configured CUDA prover client
pub fn create_cuda_prover() -> Result<Arc<CudaProver>> {
    let cuda_prover = Arc::new(ProverClient::builder().cuda().build());
    tracing::info!("Created SP1 ProverClient in CUDA mode");
    Ok(cuda_prover)
}

/// Identifies gaps not covered by the given sub-ranges in the overall range.
///
/// This function takes a vector of sub-ranges and an overall range, and returns a vector of ranges
/// that are not covered by any of the sub-ranges within the overall range.
///
/// # Arguments
///
/// * `sub_ranges` - A vector of (start, end) tuples representing the sub-ranges.
/// * `overall_start` - The start of the overall range.
/// * `overall_end` - The end of the overall range.
///
/// # Returns
///
/// A vector of (start, end) tuples representing the gaps.
///
/// # Example
///
/// ```rust
/// use op_succinct_validity::find_gaps;
///
/// let sub_ranges = vec![(10, 20), (30, 40)];
/// let gaps = find_gaps(sub_ranges, 5, 45);
/// assert_eq!(gaps, vec![(5, 10), (20, 30), (40, 45)]);
/// ```
pub fn find_gaps(sub_ranges: Vec<(u64, u64)>, overall_start: u64, overall_end: u64) -> Vec<(u64, u64)> {
    if overall_start >= overall_end {
        return vec![];
    }

    let mut ranges = sub_ranges;
    ranges.sort_by_key(|&(start, _)| start);

    let mut gaps = Vec::new();
    let mut current_pos = overall_start;

    for (start, end) in ranges {
        // Skip ranges that are completely outside the overall range
        if end <= overall_start || start >= overall_end {
            continue;
        }

        // Adjust range to be within the overall range
        let adjusted_start = start.max(overall_start);
        let adjusted_end = end.min(overall_end);

        // If there's a gap before this range, add it
        if current_pos < adjusted_start {
            gaps.push((current_pos, adjusted_start));
        }

        // Move current position to the end of this range
        current_pos = current_pos.max(adjusted_end);
    }

    // If there's a gap after the last range, add it
    if current_pos < overall_end {
        gaps.push((current_pos, overall_end));
    }

    gaps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_gaps_basic() {
        let sub_ranges = vec![(10, 20), (30, 40)];
        let gaps = find_gaps(sub_ranges, 5, 45);
        assert_eq!(gaps, vec![(5, 10), (20, 30), (40, 45)]);
    }

    #[test]
    fn test_find_gaps_no_gaps() {
        let sub_ranges = vec![(10, 20), (20, 30), (30, 40)];
        let gaps = find_gaps(sub_ranges, 10, 40);
        assert_eq!(gaps, vec![]);
    }

    #[test]
    fn test_find_gaps_overlapping() {
        let sub_ranges = vec![(10, 25), (20, 35)];
        let gaps = find_gaps(sub_ranges, 5, 45);
        assert_eq!(gaps, vec![(5, 10), (35, 45)]);
    }

    #[test]
    fn test_find_gaps_out_of_range() {
        let sub_ranges = vec![(1, 5), (50, 60)];
        let gaps = find_gaps(sub_ranges, 10, 40);
        assert_eq!(gaps, vec![(10, 40)]);
    }

    #[test]
    fn test_find_gaps_empty_input() {
        let sub_ranges = vec![];
        let gaps = find_gaps(sub_ranges, 10, 40);
        assert_eq!(gaps, vec![(10, 40)]);
    }

    #[test]
    fn test_find_gaps_invalid_overall_range() {
        let sub_ranges = vec![(10, 20)];
        let gaps = find_gaps(sub_ranges, 40, 30);
        assert_eq!(gaps, vec![]);
    }

    #[test]
    fn test_find_gaps_single_range_covers_all() {
        let sub_ranges = vec![(5, 45)];
        let gaps = find_gaps(sub_ranges, 10, 40);
        assert_eq!(gaps, vec![]);
    }
}
//! Snapping chapter starts to the nearest segment start, the first one to 0.

use crate::chapters::Chapter;
use crate::transcript::Segment;

/// Each start to its nearest segment start, then the first start to 0. Never reorders.
pub(super) fn snap(chapters: Vec<Chapter>, segments: &[Segment]) -> Vec<Chapter> {
    let mut chapters: Vec<Chapter> = chapters
        .into_iter()
        .map(|c| Chapter {
            start: snap_start(c.start, segments),
            title: c.title,
        })
        .collect();
    if let Some(first) = chapters.first_mut() {
        first.start = 0;
    }
    chapters
}

/// The start of the nearest segment (a tie goes to the earlier one), floored; a binary search
/// over the ascending segment starts. `start` itself when there are no segments.
fn snap_start(start: u64, segments: &[Segment]) -> u64 {
    let at = start as f64;
    let after = segments.partition_point(|s| s.start <= at);
    let before = after.checked_sub(1).and_then(|i| segments.get(i));
    let nearest = match (before, segments.get(after)) {
        (Some(before), Some(next)) if next.start - at < at - before.start => next,
        (Some(before), _) => before,
        (None, Some(next)) => next,
        (None, None) => return start,
    };
    nearest.start.floor() as u64
}

#[cfg(test)]
mod tests {
    use crate::chapters::validate::parse_answer;
    use crate::chapters::validate::test_support::{answer, chapter, no_limits, segments};

    #[test]
    fn snap_nearest() {
        let segs = segments(&[0.0, 100.0, 200.0, 260.0, 400.0], 500.0);
        // 3:45 = 225 is 25 s from 200 and 35 s from 260; 4:05 = 245 is 45 s from 200, 15 s from 260.
        let a = answer(&[("0:00", "A"), ("3:45", "B"), ("4:05", "C")]);
        assert_eq!(
            parse_answer(&a, &segs, 500, &no_limits()).unwrap(),
            vec![chapter(0, "A"), chapter(200, "B"), chapter(260, "C")]
        );
    }

    #[test]
    fn snap_tie_earlier() {
        let segs = segments(&[0.0, 100.0, 200.0], 300.0);
        // 2:30 = 150 is 50 s from both 100 and 200.
        let a = answer(&[("0:00", "A"), ("2:30", "B"), ("3:20", "C")]);
        assert_eq!(
            parse_answer(&a, &segs, 300, &no_limits()).unwrap(),
            vec![chapter(0, "A"), chapter(100, "B"), chapter(200, "C")]
        );
    }

    #[test]
    fn snap_before_first() {
        let segs = segments(&[30.0, 100.0, 200.0], 300.0);
        let a = answer(&[("0:00", "A"), ("0:10", "B"), ("3:20", "C")]);
        assert_eq!(
            parse_answer(&a, &segs, 300, &no_limits()).unwrap(),
            vec![chapter(0, "A"), chapter(30, "B"), chapter(200, "C")]
        );
    }

    #[test]
    fn snap_after_last() {
        let segs = segments(&[0.0, 100.0, 200.0], 300.0);
        let a = answer(&[("0:00", "A"), ("1:40", "B"), ("4:50", "C")]);
        assert_eq!(
            parse_answer(&a, &segs, 300, &no_limits()).unwrap(),
            vec![chapter(0, "A"), chapter(100, "B"), chapter(200, "C")]
        );
    }

    #[test]
    fn snap_floors() {
        let segs = segments(&[0.0, 100.7, 200.2], 300.9);
        let a = answer(&[("0:00", "A"), ("1:41", "B"), ("3:20", "C")]);
        assert_eq!(
            parse_answer(&a, &segs, 300, &no_limits()).unwrap(),
            vec![chapter(0, "A"), chapter(100, "B"), chapter(200, "C")]
        );
    }

    #[test]
    fn snap_counts_segments_with_empty_text() {
        let mut segs = segments(&[0.0, 100.0, 200.0], 300.0);
        segs[1].text = String::new();
        let a = answer(&[("0:00", "A"), ("1:50", "B"), ("3:20", "C")]);
        assert_eq!(
            parse_answer(&a, &segs, 300, &no_limits()).unwrap(),
            vec![chapter(0, "A"), chapter(100, "B"), chapter(200, "C")]
        );
    }

    #[test]
    fn first_moves_to_zero() {
        let segs = segments(&[0.0, 15.0, 100.0, 200.0], 300.0);
        let a = answer(&[("0:15", "A"), ("1:40", "B"), ("3:20", "C")]);
        assert_eq!(
            parse_answer(&a, &segs, 300, &no_limits()).unwrap(),
            vec![chapter(0, "A"), chapter(100, "B"), chapter(200, "C")]
        );
    }
}

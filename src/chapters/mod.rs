//! Chapters: the chapter type, stamps and the shared limits (ADR-0008, ADR-0009).

pub mod prompt;
pub mod render;
pub mod validate;

/// Bumped when the chapters block, repair prompt, validation or rendering changes; part of
/// the fingerprint of the chapters template only. (The artifact's own `"version"` is separate, ADR-0009.)
pub const CHAPTERS_VERSION: u32 = 1;
/// The template name.
pub const NAME: &str = "chapters";
/// The file the chapters artifact is written to.
pub const JSON_FILE: &str = "chapters.json";
/// The fewest chapters a transcript may be split into.
pub const MIN_CHAPTERS: usize = 3;
/// The shortest chapter, in seconds.
pub const MIN_SECONDS: u64 = 10;
/// The shortest transcript that can be chaptered: `MIN_CHAPTERS * MIN_SECONDS`.
pub const MIN_TRANSCRIPT_SECONDS: u64 = 30;

/// One chapter: where it starts (whole seconds) and its title.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Chapter {
    pub start: u64,
    pub title: String,
}

/// The limits a chapters template may set; `None` leaves a limit unset.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChaptersSpec {
    pub max: Option<usize>,
    pub max_chars: Option<usize>,
}

/// `h:mm:ss`, `hh:mm:ss`, `m:ss`, `mm:ss`; seconds 0-59; minutes 0-59 when hours are given;
/// checked arithmetic. `None` for anything else.
pub fn parse_stamp(s: &str) -> Option<u64> {
    let groups: Vec<&str> = s.split(':').collect();
    let digits = |g: &str, min: usize, max: usize| {
        (min..=max).contains(&g.len()) && g.bytes().all(|b| b.is_ascii_digit())
    };
    let number = |g: &str| g.parse::<u64>().ok().filter(|n| *n < 60);
    let (hours, minutes, seconds) = match groups.as_slice() {
        [m, s] if digits(m, 1, 2) && digits(s, 2, 2) => (0, number(m)?, number(s)?),
        [h, m, s] if digits(h, 1, 2) && digits(m, 2, 2) && digits(s, 2, 2) => {
            (h.parse::<u64>().ok()?, number(m)?, number(s)?)
        }
        _ => return None,
    };
    hours.checked_mul(3600)?.checked_add(minutes * 60 + seconds)
}

/// `m:ss` below 3600, `h:mm:ss` from 3600.
pub fn stamp(seconds: u64) -> String {
    if seconds < 3600 {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    } else {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds % 3600 / 60,
            seconds % 60
        )
    }
}

/// `<stamp> <title>` per chapter joined with "\n", no trailing newline. What `max_chars` measures.
pub fn lines(chapters: &[Chapter]) -> String {
    chapters
        .iter()
        .map(|c| format!("{} {}", stamp(c.start), c.title))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stamp_accepts_m_ss_zero() {
        assert_eq!(parse_stamp("0:00"), Some(0));
    }

    #[test]
    fn parse_stamp_accepts_m_ss() {
        assert_eq!(parse_stamp("4:12"), Some(252));
    }

    #[test]
    fn parse_stamp_accepts_mm_ss() {
        assert_eq!(parse_stamp("04:12"), Some(252));
    }

    #[test]
    fn parse_stamp_accepts_h_mm_ss() {
        assert_eq!(parse_stamp("1:02:03"), Some(3723));
    }

    #[test]
    fn parse_stamp_accepts_hh_mm_ss() {
        assert_eq!(parse_stamp("01:02:03"), Some(3723));
    }

    #[test]
    fn parse_stamp_accepts_zero_hours() {
        assert_eq!(parse_stamp("00:00:00"), Some(0));
    }

    #[test]
    fn parse_stamp_rejects_unit_letters() {
        assert_eq!(parse_stamp("4m12"), None);
    }

    #[test]
    fn parse_stamp_rejects_one_digit_seconds() {
        assert_eq!(parse_stamp("1:2"), None);
    }

    #[test]
    fn parse_stamp_rejects_sixty_seconds() {
        assert_eq!(parse_stamp("00:60"), None);
    }

    #[test]
    fn parse_stamp_rejects_sixty_minutes_with_hours() {
        assert_eq!(parse_stamp("1:60:00"), None);
    }

    #[test]
    fn parse_stamp_rejects_bare_seconds() {
        assert_eq!(parse_stamp("252"), None);
    }

    #[test]
    fn parse_stamp_rejects_empty_minutes() {
        assert_eq!(parse_stamp(":12"), None);
    }

    #[test]
    fn parse_stamp_rejects_four_groups() {
        assert_eq!(parse_stamp("1:02:03:04"), None);
    }

    #[test]
    fn parse_stamp_rejects_negative() {
        assert_eq!(parse_stamp("-1:00"), None);
    }

    #[test]
    fn parse_stamp_rejects_leading_space() {
        assert_eq!(parse_stamp(" 4:12"), None);
    }

    #[test]
    fn parse_stamp_rejects_huge_hours() {
        let huge = format!("{}:00:00", "9".repeat(30));
        assert_eq!(parse_stamp(&huge), None);
    }

    #[test]
    fn stamp_zero() {
        assert_eq!(stamp(0), "0:00");
    }

    #[test]
    fn stamp_252() {
        assert_eq!(stamp(252), "4:12");
    }

    #[test]
    fn stamp_3599() {
        assert_eq!(stamp(3599), "59:59");
    }

    #[test]
    fn stamp_3600() {
        assert_eq!(stamp(3600), "1:00:00");
    }

    #[test]
    fn stamp_3723() {
        assert_eq!(stamp(3723), "1:02:03");
    }

    fn chapter(start: u64, title: &str) -> Chapter {
        Chapter {
            start,
            title: title.into(),
        }
    }

    #[test]
    fn lines_two_chapters() {
        let chapters = [chapter(0, "Intro"), chapter(252, "Ownership")];
        assert_eq!(lines(&chapters), "0:00 Intro\n4:12 Ownership");
    }

    #[test]
    fn lines_empty() {
        assert_eq!(lines(&[]), "");
    }

    #[test]
    fn chapter_serializes_start_then_title() {
        let json = serde_json::to_string(&chapter(0, "Intro")).unwrap();
        assert_eq!(json, r#"{"start":0,"title":"Intro"}"#);
    }
}

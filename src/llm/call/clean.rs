/// Trims, drops a leading `<think>…</think>` block and unwraps output that is exactly one
/// fenced code block. Output with prose around a fence, or with several fences, is kept.
pub fn clean(output: &str) -> String {
    let mut text = output.trim();
    if let Some(rest) = text.strip_prefix("<think>") {
        if let Some((_, after)) = rest.split_once("</think>") {
            text = after.trim();
        }
    }
    unwrap_fence(text).unwrap_or(text).to_string()
}

fn backtick_run(line: &str) -> usize {
    line.chars().take_while(|&c| c == '`').count()
}

/// A line made only of backticks (a bare fence run), as its length.
fn bare_run(line: &str) -> Option<usize> {
    let line = line.trim();
    (!line.is_empty() && line.chars().all(|c| c == '`')).then_some(line.len())
}

/// Unwraps one fenced block: an opening run of n >= 3 backticks (plus an optional info
/// string), a last line that is a bare run of at least n, and no earlier bare run of at
/// least n (that would close the block early, so the text is several blocks).
fn unwrap_fence(text: &str) -> Option<&str> {
    let (first, rest) = text.split_once('\n')?;
    let open = backtick_run(first);
    if open < 3 {
        return None;
    }
    let (body, last) = match rest.rsplit_once('\n') {
        Some((body, last)) => (body, last),
        None => ("", rest),
    };
    if bare_run(last)? < open {
        return None;
    }
    if body.lines().any(|l| bare_run(l).is_some_and(|n| n >= open)) {
        return None;
    }
    Some(body.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_strips_think() {
        assert_eq!(clean("<think>hmm\nmore</think>\n\n  answer \n"), "answer");
    }

    #[test]
    fn clean_unwraps_fence() {
        assert_eq!(clean("```markdown\n# T\n\ntext\n```"), "# T\n\ntext");
        assert_eq!(clean("  ```\nplain\n```\n"), "plain");
        assert_eq!(clean("````\nplain\n````"), "plain");
        assert_eq!(
            clean("````markdown\n# N\n```sh\nls\n```\n````"),
            "# N\n```sh\nls\n```"
        );
    }

    #[test]
    fn clean_trims_unwrapped_fence_body() {
        assert_eq!(clean("```\n   \n```"), "");
        assert_eq!(clean("```\n\n x \n\n```"), "x");
    }

    #[test]
    fn clean_leaves_non_leading_or_unclosed_think() {
        assert_eq!(
            clean(" answer <think>x</think> "),
            "answer <think>x</think>"
        );
        assert_eq!(clean("<think>unclosed answer\n"), "<think>unclosed answer");
    }

    #[test]
    fn clean_keeps_inner_fences() {
        let prose = "Intro\n```rust\nfn a() {}\n```\nOutro";
        assert_eq!(clean(prose), prose);
        let two = "```a\nx\n```\ntext\n```b\ny\n```";
        assert_eq!(clean(two), two);
    }
}

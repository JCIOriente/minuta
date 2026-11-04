use std::sync::atomic::{AtomicUsize, Ordering};

use super::test_support::{
    chunk_total, fits, is_final, map_calls, numbered_lines, request, run_with_block, Recorder, Rng,
    BLOCK, BODY, LANG,
};
use super::*;

#[test]
fn reduce_joins_parts_in_order() {
    let input = numbered_lines(200);
    let rec = Recorder::new();
    let complete = |s: &str, u: &str| {
        rec.record(s, u);
        if is_final(s) {
            Ok("final".to_string())
        } else {
            Ok(format!("notes on {}", u.lines().next().unwrap()))
        }
    };
    let mut events = Vec::new();
    let out = generate(&request(&input, 300, 3), &complete, &mut |e| events.push(e)).unwrap();
    let total = chunk_total(&events);
    let calls = rec.calls();
    assert_eq!(
        out,
        Generated {
            text: "final".into(),
            calls: total + 1
        }
    );
    let instruction = language_instruction(LANG);
    let maps = map_calls(&calls, total);
    for (s, _) in &maps {
        assert!(s.ends_with(&format!("\n\n{instruction}")), "{s}");
    }
    let want_user = maps
        .iter()
        .enumerate()
        .map(|(i, (_, u))| {
            format!(
                "## Part {}\n\nnotes on {}",
                i + 1,
                u.lines().next().unwrap()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let (final_system, final_user) = calls.last().unwrap();
    assert_eq!(final_user, &want_user);
    assert_eq!(
        final_system,
        &format!("{BODY}\n\n{CONSOLIDATION}\n\n{instruction}")
    );
}

#[test]
fn multi_round_reduce() {
    let input = numbered_lines(400);
    let rec = Recorder::new();
    let complete = |s: &str, u: &str| {
        rec.record(s, u);
        if is_final(s) {
            Ok("final".to_string())
        } else if u.starts_with("## Part") {
            // Shrinks, but not enough to fit after one round.
            Ok("y".repeat(u.chars().count() * 3 / 4))
        } else {
            Ok("x".repeat(200))
        }
    };
    // Concurrency 1 keeps the recorded calls in order, so each round starts with part 1.
    let out = generate(&request(&input, 400, 1), &complete, &mut |_| {}).unwrap();
    let calls = rec.calls();
    let rounds = calls
        .iter()
        .filter(|(s, u)| u.starts_with("## Part") && s.contains("This is part 1 of "))
        .count();
    assert!(
        rounds >= 2,
        "expected at least two merge rounds, got {rounds}"
    );
    assert_eq!(out.text, "final");
    assert_eq!(out.calls, calls.len());
    for (s, u) in &calls {
        assert!(fits(s, u, 400), "call over budget");
    }
}

#[test]
fn merge_rounds_report_progress() {
    let input = numbered_lines(400);
    // 7 map notes need a first merge round of 4 groups, whose notes still do not fit, then
    // a second round of 2 groups, whose short notes fit the consolidation call. Each round
    // reports its start (0 done), then every finished group.
    let complete = |s: &str, u: &str| {
        if is_final(s) {
            Ok("final".to_string())
        } else if u.contains('y') {
            Ok("z".to_string())
        } else if u.starts_with("## Part") {
            Ok("y".repeat(200))
        } else {
            Ok("x".repeat(200))
        }
    };
    let mut events = Vec::new();
    let out = generate(&request(&input, 400, 3), &complete, &mut |e| events.push(e)).unwrap();
    let chunk = |done| Event::Chunk { done, total: 7 };
    let merge = |round, done, total| Event::Merge { round, done, total };
    let want = vec![
        chunk(1),
        chunk(2),
        chunk(3),
        chunk(4),
        chunk(5),
        chunk(6),
        chunk(7),
        Event::Consolidating,
        merge(1, 0, 4),
        merge(1, 1, 4),
        merge(1, 2, 4),
        merge(1, 3, 4),
        merge(1, 4, 4),
        merge(2, 0, 2),
        merge(2, 1, 2),
        merge(2, 2, 2),
        // The final consolidation call.
        Event::Consolidating,
    ];
    assert_eq!(events, want);
    assert_eq!(
        out,
        Generated {
            text: "final".into(),
            calls: 7 + 4 + 2 + 1
        }
    );
}

#[test]
fn non_shrinking_notes_fail() {
    let input = numbered_lines(200);
    let complete = |_: &str, u: &str| {
        if u.starts_with("## Part") {
            Ok(u.to_string())
        } else {
            Ok("x".repeat(200))
        }
    };
    let err = generate(&request(&input, 400, 2), &complete, &mut |_| {}).unwrap_err();
    assert!(
        matches!(
            err,
            Error::Template { ref name, ref message }
                if name.is_empty() && message == "notes do not shrink below the context budget"
        ),
        "{err}"
    );
}

#[test]
fn single_call_final_block_before_language() {
    let (_, _, finals, _) = run_with_block(&request("hello", 1000, 1));
    assert_eq!(
        finals[0].0,
        format!("{BODY}\n\n{BLOCK}\n\n{}", language_instruction(LANG))
    );
}

#[test]
fn finish_used_for_single_call() {
    let (out, calls, finals, _) = run_with_block(&request("hello", 1000, 1));
    assert_eq!(
        out,
        Generated {
            text: "final".into(),
            calls: 1
        }
    );
    assert!(calls.is_empty(), "complete was called: {calls:?}");
    assert_eq!(finals.len(), 1);
    assert_eq!(finals[0].1, "hello");
}

#[test]
fn map_prompts_have_no_final_block() {
    let input = numbered_lines(200);
    let (_, calls, _, events) = run_with_block(&request(&input, 300, 3));
    assert!(chunk_total(&events) > 1);
    for (s, _) in &calls {
        assert!(!s.contains(BLOCK), "map prompt has the final block: {s}");
    }
}

#[test]
fn consolidation_has_final_block() {
    let input = numbered_lines(200);
    let (out, calls, finals, events) = run_with_block(&request(&input, 300, 3));
    let total = chunk_total(&events);
    assert_eq!(
        out,
        Generated {
            text: "final".into(),
            calls: total + 1
        }
    );
    assert_eq!(calls.len(), total);
    assert!(calls.iter().all(|(s, _)| !is_final(s)), "{calls:?}");
    assert_eq!(finals.len(), 1);
    let (system, user) = &finals[0];
    assert_eq!(
        system,
        &format!(
            "{BODY}\n\n{CONSOLIDATION}\n\n{BLOCK}\n\n{}",
            language_instruction(LANG)
        )
    );
    assert_eq!(user, &join_notes(&vec!["n".to_string(); total]));
}

#[test]
fn fit_boundary_counts_final_block() {
    let context = 1000;
    let plain = format!("{BODY}\n\n{}", language_instruction(LANG));
    let with_block = format!("{BODY}\n\n{BLOCK}\n\n{}", language_instruction(LANG));
    let room = |system: &str| (context - context / 4 - estimate_tokens(system)) * 3;
    let exact = "x".repeat(room(&with_block));
    let fits_only_without = "x".repeat(room(&plain));
    assert!(fits(&plain, &fits_only_without, context));
    assert!(!fits(&with_block, &fits_only_without, context));
    let cases = [
        (exact.clone(), true),
        (format!("{exact}x"), false),
        (fits_only_without, false),
    ];
    for (input, one_call) in cases {
        let (out, _, _, _) = run_with_block(&request(&input, context, 1));
        assert_eq!(out.calls == 1, one_call, "input of {} chars", input.len());
    }
}

#[test]
fn final_with_block_never_exceeds_budget() {
    let mut ok = 0;
    let mut chunked = 0;
    for case in 0..300u64 {
        let mut rng = Rng(case.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let context = rng.range(60, 3000);
        let body_words = rng.range(0, 60);
        let body = rng.words(body_words, 12).join(" ");
        let block_words = rng.range(1, 40);
        // Upper case keeps the block from occurring by chance in the body or input.
        let block = format!("FIELDS: {}", rng.words(block_words, 12).join(" "));
        let line_count = rng.range(1, 80);
        let lines: Vec<String> = (0..line_count)
            .map(|_| {
                let n = rng.range(0, 30);
                let max_len = if rng.range(0, 8) == 0 { 400 } else { 15 };
                rng.words(n, max_len).join(" ")
            })
            .collect();
        let input = lines.join("\n");
        let concurrency = rng.range(1, 5);
        let req = Request {
            body: &body,
            output_lang: LANG,
            input: &input,
            context_tokens: context,
            concurrency,
        };
        let rec = Recorder::new();
        let complete = |s: &str, u: &str| {
            rec.record(s, u);
            Ok("note".to_string())
        };
        let mut finals = Vec::new();
        let mut finish = |s: &str, u: &str, _: &mut dyn FnMut(Event)| {
            finals.push((s.to_string(), u.to_string()));
            Ok("final".to_string())
        };
        let result = generate_with(&req, Some(&block), &complete, &mut finish, &mut |_| {});
        let calls = rec.calls();
        for (s, u) in &calls {
            assert!(fits(s, u, context), "case {case}: map call over budget");
            assert!(!s.contains(&block), "case {case}: map call has the block");
        }
        for (s, u) in &finals {
            assert!(fits(s, u, context), "case {case}: final call over budget");
            assert!(
                s.contains(&block),
                "case {case}: final call lacks the block"
            );
        }
        match result {
            Ok(out) => {
                ok += 1;
                if !calls.is_empty() {
                    chunked += 1;
                }
                assert_eq!(finals.len(), 1, "case {case}");
                assert_eq!(out.calls, calls.len() + 1, "case {case}");
            }
            Err(Error::Template { name, .. }) => assert!(name.is_empty(), "case {case}"),
            Err(e) => panic!("case {case}: unexpected error {e}"),
        }
    }
    assert!(ok > 150, "only {ok} cases succeeded");
    assert!(chunked > 50, "only {chunked} cases were chunked");
}

#[test]
fn oversized_final_block_fails_before_map_calls() {
    let input = numbered_lines(200);
    let block = "word ".repeat(200);
    let count = AtomicUsize::new(0);
    let complete = |_: &str, _: &str| {
        count.fetch_add(1, Ordering::SeqCst);
        Ok("n".to_string())
    };
    let mut finish = |_: &str, _: &str, _: &mut dyn FnMut(Event)| Ok("final".to_string());
    let err = generate_with(
        &request(&input, 300, 1),
        Some(&block),
        &complete,
        &mut finish,
        &mut |_| {},
    )
    .unwrap_err();
    assert!(
        matches!(
            err,
            Error::Template { ref name, ref message }
                if name.is_empty()
                    && message == "the prompt alone does not fit the context budget of 300 tokens"
        ),
        "{err}"
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn finish_events_forwarded() {
    let repair = Event::Repair {
        attempt: 1,
        total: 2,
    };
    let chunked = numbered_lines(200);
    let cases = [("hello".to_string(), false), (chunked, true)];
    for (input, is_chunked) in cases {
        let complete = |_: &str, _: &str| Ok("n".to_string());
        let mut finish = |_: &str, _: &str, on_event: &mut dyn FnMut(Event)| {
            on_event(repair);
            Ok("final".to_string())
        };
        let mut events = Vec::new();
        generate_with(
            &request(&input, 300, 3),
            Some(BLOCK),
            &complete,
            &mut finish,
            &mut |e| events.push(e),
        )
        .unwrap();
        let total = chunk_total(&events);
        let mut want: Vec<Event> = (1..=total)
            .map(|done| Event::Chunk { done, total })
            .collect();
        if is_chunked {
            want.push(Event::Consolidating);
        }
        want.push(repair);
        assert_eq!(events, want, "chunked: {is_chunked}");
    }
}

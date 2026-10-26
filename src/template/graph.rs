//! Template dependency order.

use crate::error::{Error, Result};

/// `deps_of(name)`: `None` = unknown template; `Some(deps)` = the templates it reads, in order
/// (empty: it reads the transcript).
type DepsOf<'a> = &'a dyn Fn(&str) -> Option<Vec<String>>;

/// The requested templates plus everything they read, dependencies first, each template's
/// dependencies in list order. Where dependencies do not force an order, the request order is
/// kept.
pub fn order(requested: &[String], deps_of: DepsOf) -> Result<Vec<String>> {
    let mut done = Vec::new();
    let mut stack = Vec::new();
    for name in requested {
        if deps_of(name).is_none() {
            return Err(Error::Usage(format!("unknown template {name}")));
        }
        visit(name, deps_of, &mut done, &mut stack)?;
    }
    Ok(done)
}

fn visit(
    name: &str,
    deps_of: DepsOf,
    done: &mut Vec<String>,
    stack: &mut Vec<String>,
) -> Result<()> {
    if done.iter().any(|d| d == name) {
        return Ok(());
    }
    if let Some(start) = stack.iter().position(|s| s == name) {
        let mut path: Vec<&str> = stack[start..].iter().map(String::as_str).collect();
        path.push(name);
        return Err(Error::Usage(format!(
            "template cycle: {}",
            path.join(" -> ")
        )));
    }
    // `order` and the recursion below only visit names `deps_of` knows.
    if let Some(deps) = deps_of(name) {
        stack.push(name.to_string());
        for dep in deps {
            if deps_of(&dep).is_none() {
                return Err(Error::Usage(format!(
                    "template {name}: unknown input template {dep}"
                )));
            }
            visit(&dep, deps_of, done, stack)?;
        }
        stack.pop();
    }
    done.push(name.to_string());
    Ok(())
}

/// `of` plus every template in `order` that transitively depends on it, in `order`.
/// `order` must be topological (as returned by [`order`]).
/// `of` must be in `order`; callers check membership first (an unknown name returns an empty Vec).
pub fn dependents(order: &[String], of: &str, deps_of: DepsOf) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in order {
        let depends = deps_of(name).is_some_and(|deps| deps.iter().any(|d| out.contains(d)));
        if name == of || depends {
            out.push(name.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(pairs: &[(&str, &[&str])]) -> impl Fn(&str) -> Option<Vec<String>> {
        let map: Vec<(String, Vec<String>)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.iter().map(|d| d.to_string()).collect()))
            .collect();
        move |name| map.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn usage(r: Result<Vec<String>>) -> String {
        match r {
            Err(Error::Usage(m)) => m,
            other => panic!("expected usage error, got {other:?}"),
        }
    }

    #[test]
    fn chain_runs_dependency_first() {
        let f = lookup(&[("a", &[]), ("b", &["a"])]);
        assert_eq!(order(&names(&["b"]), &f).unwrap(), names(&["a", "b"]));
    }

    #[test]
    fn diamond_runs_each_once() {
        let f = lookup(&[("a", &[]), ("b", &["a"]), ("c", &["a"]), ("d", &["b"])]);
        // d needs b only in this lookup; request c and d to cover the shared root
        assert_eq!(
            order(&names(&["d", "c"]), &f).unwrap(),
            names(&["a", "b", "d", "c"])
        );
    }

    #[test]
    fn independent_keep_request_order() {
        let f = lookup(&[("a", &[]), ("b", &[]), ("c", &[])]);
        assert_eq!(
            order(&names(&["c", "a", "b"]), &f).unwrap(),
            names(&["c", "a", "b"])
        );
    }

    #[test]
    fn unknown_requested_is_usage() {
        let f = lookup(&[("a", &[])]);
        assert_eq!(usage(order(&names(&["z"]), &f)), "unknown template z");
    }

    #[test]
    fn unknown_input_is_usage() {
        let f = lookup(&[("b", &["z"])]);
        assert_eq!(
            usage(order(&names(&["b"]), &f)),
            "template b: unknown input template z"
        );
    }

    #[test]
    fn self_cycle_named() {
        let f = lookup(&[("a", &["a"])]);
        assert_eq!(usage(order(&names(&["a"]), &f)), "template cycle: a -> a");
    }

    #[test]
    fn cycle_named_with_path() {
        let f = lookup(&[("a", &["b"]), ("b", &["a"])]);
        assert_eq!(
            usage(order(&names(&["a"]), &f)),
            "template cycle: a -> b -> a"
        );
    }

    #[test]
    fn cycle_path_skips_entry_chain() {
        let f = lookup(&[("c", &["a"]), ("a", &["b"]), ("b", &["a"])]);
        assert_eq!(
            usage(order(&names(&["c"]), &f)),
            "template cycle: a -> b -> a"
        );
    }

    #[test]
    fn dependents_transitive() {
        let f = lookup(&[
            ("a", &[]),
            ("b", &["a"]),
            ("c", &["b"]),
            ("x", &[]),
            ("d", &["x"]),
        ]);
        let ord = names(&["a", "x", "b", "d", "c"]);
        assert_eq!(dependents(&ord, "a", &f), names(&["a", "b", "c"]));
        assert_eq!(dependents(&ord, "x", &f), names(&["x", "d"]));
        assert_eq!(dependents(&ord, "c", &f), names(&["c"]));
    }

    #[test]
    fn order_two_deps() {
        let f = lookup(&[
            ("summary", &[]),
            ("chapters", &[]),
            ("youtube", &["summary", "chapters"]),
        ]);
        assert_eq!(
            order(&names(&["youtube"]), &f).unwrap(),
            names(&["summary", "chapters", "youtube"])
        );
    }

    #[test]
    fn unknown_second_dep_is_usage() {
        let f = lookup(&[("summary", &[]), ("youtube", &["summary", "chapters"])]);
        assert_eq!(
            usage(order(&names(&["youtube"]), &f)),
            "template youtube: unknown input template chapters"
        );
    }

    #[test]
    fn dependents_through_field() {
        let f = lookup(&[
            ("summary", &[]),
            ("chapters", &[]),
            ("youtube", &["summary", "chapters"]),
            ("linkedin", &["summary"]),
        ]);
        let ord = names(&["summary", "chapters", "youtube", "linkedin"]);
        assert_eq!(
            dependents(&ord, "chapters", &f),
            names(&["chapters", "youtube"])
        );
    }

    #[test]
    fn dependents_through_input() {
        let f = lookup(&[
            ("summary", &[]),
            ("chapters", &[]),
            ("youtube", &["summary", "chapters"]),
            ("toc", &["chapters"]),
        ]);
        let ord = names(&["summary", "chapters", "youtube", "toc"]);
        assert_eq!(
            dependents(&ord, "chapters", &f),
            names(&["chapters", "youtube", "toc"])
        );
    }

    #[test]
    fn cycle_through_second_edge() {
        let f = lookup(&[("a", &["x", "chapters"]), ("x", &[]), ("chapters", &["a"])]);
        assert_eq!(
            usage(order(&names(&["a"]), &f)),
            "template cycle: a -> chapters -> a"
        );
    }
}

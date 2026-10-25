//! Template dependency order.

use crate::error::{Error, Result};

/// `input_of(name)`: `None` = unknown template, `Some(None)` = reads the transcript,
/// `Some(Some(dep))` = reads the output of template `dep`.
type InputOf<'a> = &'a dyn Fn(&str) -> Option<Option<String>>;

/// The requested templates plus everything their `input` reaches, dependencies first.
/// Where dependencies do not force an order, the request order is kept.
pub fn order(requested: &[String], input_of: InputOf) -> Result<Vec<String>> {
    let mut done = Vec::new();
    let mut stack = Vec::new();
    for name in requested {
        if input_of(name).is_none() {
            return Err(Error::Usage(format!("unknown template {name}")));
        }
        visit(name, input_of, &mut done, &mut stack)?;
    }
    Ok(done)
}

fn visit(
    name: &str,
    input_of: InputOf,
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
    // `order` and the recursion below only visit names `input_of` knows.
    if let Some(Some(dep)) = input_of(name) {
        if input_of(&dep).is_none() {
            return Err(Error::Usage(format!(
                "template {name}: unknown input template {dep}"
            )));
        }
        stack.push(name.to_string());
        visit(&dep, input_of, done, stack)?;
        stack.pop();
    }
    done.push(name.to_string());
    Ok(())
}

/// `of` plus every template in `order` that transitively depends on it, in `order`.
/// `order` must be topological (as returned by [`order`]).
pub fn dependents(order: &[String], of: &str, input_of: InputOf) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in order {
        let depends = matches!(input_of(name), Some(Some(dep)) if out.contains(&dep));
        if name == of || depends {
            out.push(name.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(pairs: &[(&str, Option<&str>)]) -> impl Fn(&str) -> Option<Option<String>> {
        let map: Vec<(String, Option<String>)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.map(str::to_string)))
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
        let f = lookup(&[("a", None), ("b", Some("a"))]);
        assert_eq!(order(&names(&["b"]), &f).unwrap(), names(&["a", "b"]));
    }

    #[test]
    fn diamond_runs_each_once() {
        let f = lookup(&[
            ("a", None),
            ("b", Some("a")),
            ("c", Some("a")),
            ("d", Some("b")),
        ]);
        // d needs b only in this lookup; request c and d to cover the shared root
        assert_eq!(
            order(&names(&["d", "c"]), &f).unwrap(),
            names(&["a", "b", "d", "c"])
        );
    }

    #[test]
    fn independent_keep_request_order() {
        let f = lookup(&[("a", None), ("b", None), ("c", None)]);
        assert_eq!(
            order(&names(&["c", "a", "b"]), &f).unwrap(),
            names(&["c", "a", "b"])
        );
    }

    #[test]
    fn unknown_requested_is_usage() {
        let f = lookup(&[("a", None)]);
        assert_eq!(usage(order(&names(&["z"]), &f)), "unknown template z");
    }

    #[test]
    fn unknown_input_is_usage() {
        let f = lookup(&[("b", Some("z"))]);
        assert_eq!(
            usage(order(&names(&["b"]), &f)),
            "template b: unknown input template z"
        );
    }

    #[test]
    fn self_cycle_named() {
        let f = lookup(&[("a", Some("a"))]);
        assert_eq!(usage(order(&names(&["a"]), &f)), "template cycle: a -> a");
    }

    #[test]
    fn cycle_named_with_path() {
        let f = lookup(&[("a", Some("b")), ("b", Some("a"))]);
        assert_eq!(
            usage(order(&names(&["a"]), &f)),
            "template cycle: a -> b -> a"
        );
    }

    #[test]
    fn cycle_path_skips_entry_chain() {
        let f = lookup(&[("c", Some("a")), ("a", Some("b")), ("b", Some("a"))]);
        assert_eq!(
            usage(order(&names(&["c"]), &f)),
            "template cycle: a -> b -> a"
        );
    }

    #[test]
    fn dependents_transitive() {
        let f = lookup(&[
            ("a", None),
            ("b", Some("a")),
            ("c", Some("b")),
            ("x", None),
            ("d", Some("x")),
        ]);
        let ord = names(&["a", "x", "b", "d", "c"]);
        assert_eq!(dependents(&ord, "a", &f), names(&["a", "b", "c"]));
        assert_eq!(dependents(&ord, "x", &f), names(&["x", "d"]));
        assert_eq!(dependents(&ord, "c", &f), names(&["c"]));
    }
}

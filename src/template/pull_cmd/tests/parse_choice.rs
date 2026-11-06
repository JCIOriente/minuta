use super::*;

#[test]
fn parse_choice_accepts_numbers_in_the_order_given() {
    assert_eq!(parse_choice("1", 3).unwrap(), vec![0]);
    assert_eq!(parse_choice("1,3", 3).unwrap(), vec![0, 2]);
    assert_eq!(parse_choice("3 1", 3).unwrap(), vec![2, 0]);
    assert_eq!(parse_choice(" 2 , 3 ", 3).unwrap(), vec![1, 2]);
}

#[test]
fn parse_choice_drops_repeats() {
    assert_eq!(parse_choice("1 1,1", 3).unwrap(), vec![0]);
}

#[test]
fn parse_choice_a_means_all() {
    for text in ["a", "A", "all", "All", " a "] {
        assert_eq!(parse_choice(text, 3).unwrap(), vec![0, 1, 2], "{text:?}");
    }
}

#[test]
fn parse_choice_nothing_means_nothing() {
    for text in ["", "  ", "\t", ",", " , "] {
        assert_eq!(
            parse_choice(text, 3).unwrap(),
            Vec::<usize>::new(),
            "{text:?}"
        );
    }
}

#[test]
fn parse_choice_rejects_anything_else() {
    for (text, bad) in [
        ("0", "0"),
        ("4", "4"),
        ("x", "x"),
        ("1,x", "x"),
        ("-1", "-1"),
        ("1-3", "1-3"),
        ("a 2", "a"),
        ("1.5", "1.5"),
    ] {
        match parse_choice(text, 3) {
            Err(Error::Usage(m)) => assert_eq!(
                m,
                format!("invalid choice {bad:?} (enter numbers from 1 to 3, 'a' for all, or nothing to cancel)"),
                "{text:?}"
            ),
            other => panic!("{text:?}: expected usage error, got {other:?}"),
        }
    }
}

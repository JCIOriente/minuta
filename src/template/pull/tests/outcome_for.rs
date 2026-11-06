use super::*;

#[test]
fn outcome_for_new_is_created_with_or_without_force() {
    assert_eq!(outcome_for(State::New, false), Outcome::Created);
    assert_eq!(outcome_for(State::New, true), Outcome::Created);
}

#[test]
fn outcome_for_update_available_is_updated_with_or_without_force() {
    assert_eq!(outcome_for(State::UpdateAvailable, false), Outcome::Updated);
    assert_eq!(outcome_for(State::UpdateAvailable, true), Outcome::Updated);
}

#[test]
fn outcome_for_modified_depends_on_force() {
    assert_eq!(outcome_for(State::Modified, false), Outcome::KeptModified);
    assert_eq!(outcome_for(State::Modified, true), Outcome::Overwrote);
}

#[test]
fn outcome_for_up_to_date_is_kept_with_or_without_force() {
    assert_eq!(outcome_for(State::UpToDate, false), Outcome::KeptUpToDate);
    assert_eq!(outcome_for(State::UpToDate, true), Outcome::KeptUpToDate);
}

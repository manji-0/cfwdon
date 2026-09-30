use crate::polls::validate_poll_vote_submission;

#[test]
fn validate_poll_vote_submission_rejects_repeat_votes() {
    let error = validate_poll_vote_submission(1, true, 2).unwrap_err();
    assert_eq!(error, "you have already voted in this poll");
}

#[test]
fn validate_poll_vote_submission_rejects_multi_choice_for_single_choice_poll() {
    let error = validate_poll_vote_submission(0, false, 2).unwrap_err();
    assert_eq!(error, "poll does not allow multiple choices");
}

use serde_json::{Value, json};

use super::{bounded_summary, cut_point, fold, summary_request, threshold, used_tokens};

fn round(index: usize, output: &str) -> [Value; 2] {
    [
        json!({"role": "assistant", "content": "", "tool_calls": [{"id": format!("call-{index}"), "type": "function",
            "function": {"name": "run_workspace_command", "arguments": format!("{{\"command\":\"step {index}\"}}")}}]}),
        json!({"role": "tool", "tool_call_id": format!("call-{index}"), "content": output}),
    ]
}

fn turn(rounds: usize, output: &str) -> Vec<Value> {
    let mut messages = vec![
        json!({"role": "system", "content": "room rules"}),
        json!({"role": "user", "content": "fix the build"}),
    ];
    for index in 0..rounds {
        messages.extend(round(index, output));
    }
    messages
}

#[test]
fn compaction_starts_at_three_quarters_of_the_room_left_after_the_reply() {
    assert_eq!(threshold(32_768, 4_096), 21_504);
    assert_eq!(threshold(1_000, 65_536), 0);
}

#[test]
fn usage_reported_by_the_provider_counts_and_only_later_parts_are_estimated() {
    let messages = turn(3, &"x".repeat(3_000));
    let estimated = used_tokens(&messages, None);
    // The report covers everything up to the last assistant message.
    let covered = messages.len() - 1;
    let measured = used_tokens(&messages, Some((covered, 50_000)));
    assert!(measured > 50_000 && measured < 50_000 + estimated);
    // A report from before a fold no longer lines up and is ignored.
    assert_eq!(
        used_tokens(&messages, Some((messages.len() + 5, 50_000))),
        estimated
    );
}

#[test]
fn folding_keeps_rules_input_and_whole_recent_rounds() {
    let mut messages = turn(40, &"x".repeat(12_000));
    let cut = cut_point(&messages, 2, 100_000).unwrap_or_else(|| panic!("old rounds to fold"));
    assert_eq!(messages[cut]["role"], "assistant");
    assert!(
        cut < messages.len() - 2,
        "more than the latest round is kept"
    );
    let request = summary_request(&messages, 2, cut);
    let asked = request[1]["content"].as_str().unwrap_or_default();
    assert!(asked.starts_with("Task:\nfix the build"));
    assert!(asked.contains("Called run_workspace_command({\"command\":\"step 0\"})"));
    let kept = messages.len() - cut;
    fold(&mut messages, 2, cut, "built twice, test 3 fails");
    assert_eq!(messages[0]["content"], "room rules");
    assert_eq!(messages[1]["content"], "fix the build");
    assert!(
        messages[2]["content"]
            .as_str()
            .unwrap_or_default()
            .ends_with("built twice, test 3 fails")
    );
    assert_eq!(messages.len(), 3 + kept);
    assert_eq!(messages[3]["role"], "assistant");
}

#[test]
fn a_single_round_has_nothing_older_to_fold() {
    let messages = turn(1, &"x".repeat(200_000));
    assert_eq!(cut_point(&messages, 2, 100_000), None);
}

#[test]
fn a_long_summary_is_kept_from_its_start_and_an_empty_one_is_refused() {
    let long = format!("goal{}", "x".repeat(20_000));
    let kept = bounded_summary(&long).unwrap_or_default();
    assert_eq!(kept.chars().count(), 12_000);
    assert!(kept.starts_with("goal"));
    assert_eq!(bounded_summary("  \n "), None);
}

#[test]
fn an_earlier_summary_alone_is_not_folded_again() {
    let mut messages = turn(1, &"x".repeat(200_000));
    messages.insert(
        2,
        json!({"role": "user", "content": "Summary of the earlier work"}),
    );
    assert_eq!(cut_point(&messages, 2, 100_000), None);
}

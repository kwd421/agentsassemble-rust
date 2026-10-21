use serde_json::{Value, json};

use super::{cut_point, fold, needs_compaction, summary_request};

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
fn a_turn_compacts_only_near_the_context_window() {
    let small = turn(2, "ok");
    assert!(!needs_compaction(&small, 131_072, 4_096));
    let large = turn(40, &"x".repeat(12_000));
    assert!(needs_compaction(&large, 131_072, 4_096));
    // A long reply limit reserves more room than pi's default.
    assert!(needs_compaction(&small, 1_000, 65_536));
}

#[test]
fn folding_keeps_rules_input_and_whole_recent_rounds() {
    let mut messages = turn(40, &"x".repeat(12_000));
    let cut = cut_point(&messages, 2).unwrap_or_else(|| panic!("old rounds to fold"));
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
    assert_eq!(cut_point(&messages, 2), None);
}

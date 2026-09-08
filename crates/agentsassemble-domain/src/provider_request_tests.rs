use super::*;

fn question(id: &str, secret: bool) -> ProviderRequestQuestion {
    ProviderRequestQuestion {
        id: id.to_owned(),
        header: String::new(),
        question: "Answer".to_owned(),
        options: vec![],
        multiple: false,
        is_other: true,
        is_secret: secret,
    }
}

fn request(prompt: ProviderRequestPrompt, kind: ProviderRequestKind) -> ProviderRequest {
    ProviderRequest {
        provider_request_id: Uuid::new_v4(),
        request_kind: kind,
        title: "Input".to_owned(),
        description: String::new(),
        timeout_seconds: 600,
        prompt,
    }
}

#[test]
fn secret_answers_never_enter_durable_resolution_and_all_questions_require_exact_answers() {
    let request = request(
        ProviderRequestPrompt::Answers {
            questions: vec![question("public", false), question("secret", true)],
        },
        ProviderRequestKind::UserInput,
    );
    let mut answers = BTreeMap::from([
        ("public".to_owned(), vec!["visible answer".to_owned()]),
        (
            "secret".to_owned(),
            vec!["fixture-private-answer".to_owned()],
        ),
    ]);
    let resolution = ProviderRequestResolution::Answers {
        answers: answers.clone(),
    };
    let durable = request
        .durable_resolution(&resolution)
        .unwrap_or_else(|| panic!("valid answers rejected"));
    let json =
        serde_json::to_value(durable).unwrap_or_else(|_| panic!("projection encoding failed"));
    assert_eq!(
        json["answers"],
        serde_json::json!({"public":["visible answer"]})
    );
    assert_eq!(
        json["secret_answered_question_ids"],
        serde_json::json!(["secret"])
    );
    assert!(!json.to_string().contains("fixture-private-answer"));
    answers.insert("extra".to_owned(), vec!["no".to_owned()]);
    assert!(
        request
            .durable_resolution(&ProviderRequestResolution::Answers {
                answers: answers.clone()
            })
            .is_none()
    );
    answers.remove("extra");
    answers.remove("public");
    assert!(
        request
            .durable_resolution(&ProviderRequestResolution::Answers { answers })
            .is_none()
    );
}

#[test]
fn offered_choices_and_external_action_urls_are_checked_before_resolution() {
    let option = ProviderRequestOption {
        id: "allow".to_owned(),
        label: "Allow once".to_owned(),
        kind: "allow_once".to_owned(),
        description: String::new(),
    };
    let mut permission = request(
        ProviderRequestPrompt::Option {
            options: vec![option.clone()],
        },
        ProviderRequestKind::Permission,
    );
    assert!(
        permission
            .durable_resolution(&ProviderRequestResolution::Option {
                option_id: "unknown".to_owned()
            })
            .is_none()
    );
    assert!(
        permission
            .durable_resolution(&ProviderRequestResolution::Option {
                option_id: "allow".to_owned()
            })
            .is_some()
    );
    permission.prompt = ProviderRequestPrompt::Option {
        options: vec![option.clone(), option],
    };
    assert!(!permission.is_valid());
    for (url, valid) in [
        ("https://example.test/action", true),
        ("http://example.test/action", false),
        ("https://credential@example.test/action", false),
        ("javascript:alert(1)", false),
    ] {
        let action = request(
            ProviderRequestPrompt::Acknowledge {
                action_url: Some(url.to_owned()),
            },
            ProviderRequestKind::ExternalAction,
        );
        assert_eq!(action.is_valid(), valid);
    }
}

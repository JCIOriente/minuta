use super::*;
use crate::test_support::fake_http::{FakeHttp, Route};

#[test]
fn endpoint_finish_length_is_not_transient() {
    for content in [json!("cut"), json!(""), Value::Null] {
        let fake = server(move |_| reply_with(content.clone(), "length"));
        assert_eq!(
            llm_err(complete(&fake).unwrap_err()),
            (
                "response cut off at the context limit; give the model a larger context on the \
                 server, or lower endpoints.box.context_tokens"
                    .to_string(),
                false
            )
        );
    }
}

#[test]
fn endpoint_finish_content_filter_is_not_transient() {
    let fake = server(|_| reply_with(json!("half a docu"), "content_filter"));
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        (
            "response ended early (finish_reason content_filter)".to_string(),
            false
        )
    );
}

#[test]
fn endpoint_finish_error_is_transient() {
    let fake = server(|_| reply_with(json!("half a docu"), "error"));
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        (
            "response ended early (finish_reason error)".to_string(),
            true
        )
    );
}

#[test]
fn endpoint_finish_unknown_reason_is_not_transient() {
    for reason in ["abort", "insufficient_system_resource"] {
        let fake = server(move |_| reply_with(json!("half a docu"), reason));
        assert_eq!(
            llm_err(complete(&fake).unwrap_err()),
            (
                format!("response ended early (finish_reason {reason})"),
                false
            )
        );
    }
}

#[test]
fn endpoint_finish_reason_with_control_characters_is_sanitized() {
    let fake = server(|_| reply_with(json!("half"), "a\u{1b}[2Jb"));
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()).0,
        "response ended early (finish_reason a[2Jb)"
    );
}

#[test]
fn endpoint_finish_normal_stops_are_accepted() {
    for reason in ["stop", "eos", "eos_token", "stop_sequence", "end_turn"] {
        let fake = server(move |_| reply_with(json!("fine"), reason));
        assert_eq!(complete(&fake).unwrap(), "fine", "{reason}");
    }
}

#[test]
fn endpoint_finish_error_shows_the_cause_on_the_choice() {
    let fake = server(|_| {
        let body = json!({"choices": [{
            "message": {"content": "half"},
            "finish_reason": "error",
            "error": {"message": "provider down"},
        }]});
        Reply::Status(200, body.to_string())
    });
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        (
            "response ended early (finish_reason error): provider down".to_string(),
            true
        )
    );
}

#[test]
fn endpoint_empty_content_shows_the_cause_on_the_choice() {
    let fake = server(|_| {
        let body = json!({"choices": [{"message": {"content": ""}, "error": "provider down"}]});
        Reply::Status(200, body.to_string())
    });
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        (
            "response has no message content: provider down".to_string(),
            true
        )
    );
}

fn reply_body(choice: Value) -> Reply {
    Reply::Status(200, json!({ "choices": [choice] }).to_string())
}

#[test]
fn endpoint_error_beside_text_without_a_reason_fails() {
    let fake = server(|_| {
        reply_body(json!({
            "message": {"content": "half a docu"},
            "finish_reason": null,
            "error": {"message": "provider down"},
        }))
    });
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        ("response ended early: provider down".to_string(), true)
    );
}

#[test]
fn endpoint_error_beside_text_with_a_normal_stop_fails() {
    let fake = server(|_| {
        reply_body(json!({
            "message": {"content": "half a docu"},
            "finish_reason": "stop",
            "error": "provider down",
        }))
    });
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        (
            "response ended early (finish_reason stop): provider down".to_string(),
            true
        )
    );
}

#[test]
fn endpoint_top_level_error_beside_text_fails() {
    let fake = server(|_| {
        let body = json!({
            "choices": [{"message": {"content": "half a docu"}, "finish_reason": "stop"}],
            "error": {"message": "provider down"},
        });
        Reply::Status(200, body.to_string())
    });
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()).0,
        "response ended early (finish_reason stop): provider down"
    );
}

#[test]
fn endpoint_finish_reason_that_is_not_a_string_is_unknown() {
    let fake = server(|_| reply_body(json!({"message": {"content": "half"}, "finish_reason": 3})));
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        ("response ended early (finish_reason 3)".to_string(), false)
    );
}

#[test]
fn endpoint_refusal_is_shown() {
    let fake = server(|_| {
        reply_body(json!({
            "message": {"content": null, "refusal": "I can't help with that"},
            "finish_reason": "stop",
        }))
    });
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        (
            "response has no message content: I can't help with that".to_string(),
            true
        )
    );
}

#[test]
fn endpoint_finish_content_filter_with_a_refusal_is_not_transient() {
    let fake = server(|_| {
        reply_body(json!({
            "message": {"content": null, "refusal": "no"},
            "finish_reason": "content_filter",
        }))
    });
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        (
            "response ended early (finish_reason content_filter): no".to_string(),
            false
        )
    );
}

#[test]
fn endpoint_refusal_beside_text_is_accepted() {
    let fake = server(|_| {
        reply_body(json!({
            "message": {"content": "whole document", "refusal": "no"},
            "finish_reason": "stop",
        }))
    });
    assert_eq!(complete(&fake).unwrap(), "whole document");
}

#[test]
fn endpoint_error_that_is_neither_object_nor_text_is_ignored() {
    for error in [json!(false), json!(0), json!([])] {
        let fake = server(move |_| {
            reply_body(json!({
                "message": {"content": "whole document"},
                "finish_reason": "stop",
                "error": error.clone(),
            }))
        });
        assert_eq!(complete(&fake).unwrap(), "whole document");
    }
}

#[test]
fn endpoint_error_without_a_message_beside_text_fails() {
    let fake = server(|_| {
        reply_body(json!({
            "message": {"content": "half a docu"},
            "finish_reason": "stop",
            "error": {"code": 502},
        }))
    });
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        (
            "response ended early (finish_reason stop)".to_string(),
            true
        )
    );
}

#[test]
fn endpoint_finish_without_a_reason_is_accepted() {
    let fake = server(|_| {
        let body = json!({"choices": [{"message": {"content": "fine"}}]});
        Reply::Status(200, body.to_string())
    });
    assert_eq!(complete(&fake).unwrap(), "fine");
}

#[test]
fn endpoint_error_in_a_200_reply_is_shown() {
    let fake = server(|_| {
        let body = json!({"error": {"code": 502, "message": "provider down"}});
        Reply::Status(200, body.to_string())
    });
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        (
            "response has no message content: provider down".to_string(),
            true
        )
    );
}

#[test]
fn endpoint_body_cut_short_is_transient() {
    let fake = server(|_| Reply::Raw("HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n{".into()));
    let (message, transient) = llm_err(complete(&fake).unwrap_err());
    assert!(message.starts_with("unreadable response: "), "{message}");
    assert!(transient);
}

#[test]
fn endpoint_empty_content_is_transient() {
    for content in [json!(""), json!("  \n"), Value::Null] {
        let fake = server(move |_| reply_with(content.clone(), "stop"));
        assert_eq!(
            llm_err(complete(&fake).unwrap_err()),
            ("response has no message content".to_string(), true)
        );
    }
}

#[test]
fn endpoint_reply_without_choices_is_transient() {
    let fake = server(|_| Reply::Status(200, "{}".into()));
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        ("response has no message content".to_string(), true)
    );
}

#[test]
fn endpoint_reply_that_is_not_json_is_not_transient() {
    let fake = server(|_| Reply::Status(200, "<html>".into()));
    let (message, transient) = llm_err(complete(&fake).unwrap_err());
    assert!(message.starts_with("unreadable response: "), "{message}");
    assert!(!transient);
}

#[test]
fn endpoint_keeps_a_non_utf8_reply() {
    let server = FakeHttp::start(vec![Route::ok(
        "/templates/chat/completions",
        b"{\"choices\":[{\"message\":{\"content\":\"a\xffb\"},\"finish_reason\":\"stop\"}]}"
            .to_vec(),
    )]);
    let backend = OpenAiCompatible::new(
        "box",
        "m",
        &server.base(),
        None,
        1000,
        Duration::from_secs(5),
    );
    assert_eq!(
        backend.complete("s", "u", &no_interrupt()).unwrap(),
        "a\u{fffd}b"
    );
}

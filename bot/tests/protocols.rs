use forex_bot::protocols::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
fn update(author: i64) -> Value {
    json!({"update_id":10,"message":{"message_id":20,"chat":{"id":-100},"from":{"id":author},"reply_to_message":{"message_id":5},"caption":"буфер 0.1","photo":[{"file_id":"small","width":10,"height":10},{"file_id":"large","width":100,"height":100}]}})
}
fn known() -> BTreeMap<i64, String> {
    BTreeMap::from([(5, "signal-1".into())])
}
#[test]
fn any_group_member_can_reply_with_caption_and_largest_photo() {
    for author in [1, 2, 3, 500] {
        let c = parse_comment(&update(author), -100, &known())
            .unwrap()
            .unwrap();
        assert_eq!(c.author_id, author);
        assert_eq!(c.signal_id, "signal-1");
        assert_eq!(c.text, "буфер 0.1");
        assert_eq!(c.files, vec!["large"]);
    }
}
#[test]
fn other_chats_and_unlinked_messages_are_ignored() {
    assert!(parse_comment(&update(1), -200, &known()).unwrap().is_none());
    assert!(parse_comment(&update(1), -100, &BTreeMap::new())
        .unwrap()
        .is_none());
}
#[test]
fn image_document_is_a_comment_even_without_text() {
    let mut u = update(1);
    u["message"].as_object_mut().unwrap().remove("photo");
    u["message"].as_object_mut().unwrap().remove("caption");
    u["message"]["document"] = json!({"file_id":"chart","mime_type":"image/png"});
    let c = parse_comment(&u, -100, &known()).unwrap().unwrap();
    assert_eq!(c.files, vec!["chart"]);
}
#[test]
fn vision_request_includes_image_and_only_supplied_context() {
    let c = Comment {
        update_id: 10,
        message_id: 20,
        chat_id: -100,
        author_id: 1,
        signal_id: "s".into(),
        text: "change".into(),
        files: vec!["chart".into()],
    };
    let r = vision_request(
        "configured-model",
        500,
        &c,
        &json!({"entry":100}),
        &json!({"stop_buffer_atr":0.05}),
        &[Image {
            mime: "image/png".into(),
            bytes: vec![1, 2, 3],
        }],
    )
    .unwrap();
    assert_eq!(r["model"], "configured-model");
    assert_eq!(r["store"], false);
    assert_eq!(r["max_output_tokens"], 500);
    assert_eq!(r["text"]["format"]["strict"], true);
    assert_eq!(r["input"][0]["content"][1]["type"], "input_image");
    assert_eq!(
        r["input"][0]["content"][1]["image_url"],
        "data:image/png;base64,AQID"
    );
    assert!(!r.to_string().contains("chat_history"));
    assert!(vision_request("configured-model", 500, &c, &json!({}), &json!({}), &[]).is_err());
}
#[test]
fn parses_completed_response_and_rejects_refusal_or_truncation() {
    let text = json!({"action":"no_change","explanation":"Принято","changes":[]}).to_string();
    let good = json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":text}]}],"usage":{"input_tokens":10,"output_tokens":4}});
    let r = parse_analysis(&good).unwrap();
    assert_eq!(r.action, "no_change");
    let mut incomplete = good.clone();
    incomplete["status"] = json!("incomplete");
    assert!(parse_analysis(&incomplete).is_err());
    let mut refusal = good;
    refusal["output"][0]["content"][0] = json!({"type":"refusal","refusal":"no"});
    assert!(parse_analysis(&refusal).is_err());
}
#[test]
fn valid_patch_changes_only_requested_parameter_and_invalid_patch_is_atomic() {
    let original: Value =
        serde_json::from_str(include_str!("../../strategy/fvg-bot-rules.json")).unwrap();
    let change = Analysis {
        action: "apply".into(),
        explanation: "буфер".into(),
        changes: vec![Change {
            path: "/parameters/stop_buffer_atr/value".into(),
            value: json!(0.1),
        }],
    };
    let patched = apply_changes(&original, &change).unwrap();
    assert_eq!(patched["parameters"]["stop_buffer_atr"]["value"], 0.1);
    assert_eq!(original["parameters"]["stop_buffer_atr"]["value"], 0.05);
    let bad = Analysis {
        action: "apply".into(),
        explanation: "bad".into(),
        changes: vec![Change {
            path: "/meta/source".into(),
            value: json!("elsewhere"),
        }],
    };
    assert!(apply_changes(&original, &bad).is_err());
    let bad = Analysis {
        action: "apply".into(),
        explanation: "bad".into(),
        changes: vec![Change {
            path: "/parameters/plan_days/value".into(),
            value: json!(-1),
        }],
    };
    assert!(apply_changes(&original, &bad).is_err());
}
#[test]
fn sequential_patches_preserve_unrelated_previous_changes() {
    let v: Value = serde_json::from_str(include_str!("../../strategy/fvg-bot-rules.json")).unwrap();
    let a = Analysis {
        action: "apply".into(),
        explanation: "a".into(),
        changes: vec![Change {
            path: "/parameters/plan_days/value".into(),
            value: json!(12),
        }],
    };
    let b = Analysis {
        action: "apply".into(),
        explanation: "b".into(),
        changes: vec![Change {
            path: "/parameters/stop_buffer_atr/value".into(),
            value: json!(0.2),
        }],
    };
    let r = apply_changes(&apply_changes(&v, &a).unwrap(), &b).unwrap();
    assert_eq!(r["parameters"]["plan_days"]["value"], 12);
    assert_eq!(r["parameters"]["stop_buffer_atr"]["value"], 0.2);
}

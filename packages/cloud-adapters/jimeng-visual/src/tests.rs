use serde_json::{json, Value};

use crate::{parameters, recovery};

fn selected(contracts: &[Value], local_id: &str) -> Value {
    let entry = contracts
        .iter()
        .find(|entry| entry["localId"] == local_id)
        .unwrap();
    json!({"modelId":entry["modelIds"][0],"capabilityKey":entry["capabilityKey"],
        "operationId":entry["operations"][0]["id"],"operationVersion":entry["operations"][0]["version"]})
}

#[test]
fn all_retained_operations_resolve_to_the_existing_visual_routes() {
    let contracts = parameters::capabilities();
    let routes = [
        ("video-720-text", "jimeng_t2v_v30", vec![]),
        (
            "video-720-first",
            "jimeng_i2v_first_v30",
            vec!["firstFrame"],
        ),
        (
            "video-720-first-last",
            "jimeng_i2v_first_tail_v30",
            vec!["firstFrame", "lastFrame"],
        ),
        (
            "video-720-camera",
            "jimeng_i2v_recamera_v30",
            vec!["firstFrame"],
        ),
        ("video-1080-text", "jimeng_t2v_v30_1080p", vec![]),
        (
            "video-1080-first",
            "jimeng_i2v_first_v30_1080",
            vec!["firstFrame"],
        ),
        (
            "video-1080-first-last",
            "jimeng_i2v_first_tail_v30_1080",
            vec!["firstFrame", "lastFrame"],
        ),
        ("video-pro-text", "jimeng_ti2v_v30_pro", vec![]),
        ("video-pro-first", "jimeng_ti2v_v30_pro", vec!["firstFrame"]),
        ("seededit", "seededit_v3.0", vec!["source"]),
        ("portrait", "i2i_portrait_photo", vec!["source"]),
    ];
    assert_eq!(contracts.len(), routes.len());
    for (id, request_key, media_fields) in routes {
        let selection = selected(&contracts, id);
        let operation = parameters::operation(&selection).unwrap();
        let mut input = json!({"prompt":"Synthetic edit","durationSeconds":10,"seed":7});
        if id == "video-720-camera" {
            input["cameraTemplate"] = json!("hitchcock_dolly_in");
            input["cameraStrength"] = json!("medium");
        }
        for field in &media_fields {
            input[*field] = json!({"mediaId":field});
        }
        let frozen = operation.material(&input).unwrap();
        assert_eq!(frozen["body"]["req_key"], request_key);
        assert_eq!(frozen["imageIds"], json!(media_fields));
        if id.starts_with("video-") {
            assert_eq!(frozen["body"]["frames"], 241);
            assert_eq!(frozen["body"]["seed"], 7);
        } else if id == "seededit" {
            assert_eq!(frozen["body"]["scale"], 0.5);
        } else {
            assert_eq!(operation.mode.submit_action(), "CVSubmitTask");
            assert_eq!(operation.mode.query_action(), "CVGetResult");
            assert_eq!(frozen["body"]["width"], 1328);
        }
    }
    assert!(!json!(contracts).to_string().contains("4.6"));
}

#[test]
fn first_last_preflight_rejects_reversed_shape_and_missing_roles() {
    let contracts = parameters::capabilities();
    let selection = selected(&contracts, "video-720-first-last");
    let operation = parameters::operation(&selection).unwrap();
    let input =
        json!({"prompt":"Synthetic edit","firstFrame":{"mediaId":"a"},"lastFrame":{"mediaId":"b"}});
    let matching = json!([
        {"mediaId":"b","kind":"image","mime":"image/png","width":800,"height":600},
        {"mediaId":"a","kind":"image","mime":"image/jpeg","width":640,"height":480},
    ]);
    operation.validate_metadata(&input, &matching).unwrap();
    let mut invalid = matching.clone();
    invalid[0]["height"] = json!(480);
    assert_eq!(
        operation.validate_metadata(&input, &invalid).unwrap_err().0,
        "/lastFrame"
    );
    assert_eq!(
        operation
            .validate_metadata(&input, &json!([matching[1]]))
            .unwrap_err()
            .0,
        "/lastFrame"
    );
    assert_eq!(
        operation
            .material(&json!({"prompt":"Fixture","firstFrame":{"mediaId":"a"}}))
            .unwrap_err()
            .0,
        "/lastFrame"
    );
}

#[test]
fn nested_original_task_ids_and_reported_usage_are_exact() {
    let raw = json!({"Result":{"data":{"task_id":7180123456789123456u64,
        "usage":{"duration_seconds":5.25,"total_tokens":12}}}});
    assert_eq!(
        recovery::task_id(&raw).as_deref(),
        Some("7180123456789123456")
    );
    assert_eq!(
        recovery::usage(&raw),
        json!({"measurements":[
        {"metric":"total_tokens","unit":"tokens","amount":"12","source":"reported"},
        {"metric":"duration","unit":"seconds","amount":"5.25","source":"reported"}]})
    );
    assert!(recovery::usage(&json!({"data":{"time_elapsed":"13s"}})).is_null());
}

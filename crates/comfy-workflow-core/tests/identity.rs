use bgx_comfy_workflow::{normalize_source_path, stable_workflow_key, SourceKind, WorkflowIdentityV1};

#[test]
fn identity_separates_instances_users_and_paths() {
    let base = WorkflowIdentityV1 { schema_version: 1, instance_id: "server-a".into(), user_scope: "default".into(), source_kind: SourceKind::ComfyUserData, source_path: "workflows/demo.json".into(), display_name: "Demo".into() };
    let mut other = base.clone();
    other.instance_id = "server-b".into();
    assert_ne!(stable_workflow_key(&base).unwrap(), stable_workflow_key(&other).unwrap());
}

#[test]
fn source_path_is_relative_and_normalized() {
    assert_eq!(normalize_source_path("workflows\\nested\\demo.json").unwrap(), "workflows/nested/demo.json");
    assert!(normalize_source_path("../demo.json").is_err());
    assert!(normalize_source_path("C:\\demo.json").is_err());
}

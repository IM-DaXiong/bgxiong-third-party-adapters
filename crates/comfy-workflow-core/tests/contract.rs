use bgx_comfy_workflow::contract::*;

#[test]
fn generated_limits_match_contract_source() {
    let raw: serde_json::Value = serde_json::from_str(include_str!(
        "../contracts/comfy-workflow-mapping.json"
    )).unwrap();
    assert_eq!(CONTRACT_VERSION as u64, raw["contractVersion"].as_u64().unwrap());
    assert_eq!(MAX_NODES as u64, raw["maxNodes"].as_u64().unwrap());
    assert!(SOURCE_KINDS.contains(&"comfyUserData"));
    assert!(SEMANTIC_ROLES.contains(&"positivePrompt"));
    assert!(TRANSFORM_KINDS.contains(&"identity"));
}

#[test]
fn asset_slot_families_match_contract_closed_set() {
    use bgx_comfy_workflow::AssetSlotFamily;
    let serialized: Vec<String> = [
        AssetSlotFamily::Named,
        AssetSlotFamily::Indexed,
        AssetSlotFamily::ExplicitNames,
        AssetSlotFamily::Flat,
    ]
    .iter()
    .map(|f| serde_json::to_value(f).unwrap().as_str().unwrap().to_string())
    .collect();
    assert_eq!(serialized, ASSET_SLOT_FAMILIES);
}

use super::*;
use std::collections::BTreeSet;
#[test]
fn unified_catalog_has_nine_unique_closed_routes() {
    let c = capabilities();
    assert_eq!(c["pluginId"], PLUGIN);
    let rows = c["capabilityContracts"].as_array().unwrap();
    assert_eq!(rows.len(), 9);
    assert!(rows.iter().all(|row| row["localId"] != "r2v-spectrum"));
    assert!(rows.iter().all(|row| row["localId"] != "soundtrack"));
    assert!(rows.iter().any(|row| row["localId"] == "r2v-soundtrack"));
    let mut ids = BTreeSet::new();
    let mut ops = BTreeSet::new();
    let mut models = BTreeSet::new();
    for row in rows {
        assert!(ids.insert(row["localId"].as_str().unwrap()));
        assert!(models.insert(row["modelIds"][0].as_str().unwrap()));
        let op = &row["operations"][0];
        let id = op["id"].as_str().unwrap();
        assert!(id.starts_with(&alloc::format!("{PLUGIN}/")));
        assert!(ops.insert(id));
        for key in ["inputSchema", "outputSchema", "resumeSchema"] {
            let reference = &op[key];
            assert!(row["schemas"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["id"] == reference["id"] && s["digest"] == reference["digest"]));
        }
    }
}

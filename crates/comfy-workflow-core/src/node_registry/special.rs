//! Versioned special-node serialization rules (CW33[L04], CW38.01–CW38.28).
//!
//! A rule applies only to one exact `(class_type, package, version, definition_digest)` tuple. A class
//! name alone never selects a rule, an unknown package version never selects a rule, and a changed
//! definition digest never selects a rule: each of those is reported as an explicit
//! missing-evidence outcome by [`lookup_special_rule`] so the capability resolver can block instead of
//! guessing.
//!
//! A rule may be registered only when the class has oracle evidence (CW03 pinned package/version,
//! CW04 official API export, CW05 regression capture) and the generic serializer profile (CW31) was
//! shown NOT to cover the class. Classes whose evidence is still missing are listed in
//! [`PENDING_SPECIAL_CLASSES`]; that list carries no slots and grants no capability.

#[cfg(target_arch = "wasm32")]
use alloc::{string::String, string::ToString, vec::Vec};

/// Bump whenever a rule is added, removed, or changes meaning. Snapshots compiled with an older value
/// are stale (plan §4.9). Previous core layout registry value was 3 (`node_registry::RULE_REGISTRY_VERSION`).
// v6: LTX frame-guide conditioning channels follow actual output slots. Invalidate legacy
// automatic mappings that previously fell back to text-node ID ordering.
pub const RULE_REGISTRY_VERSION: u32 = 6;

/// Package identity of the node class as reported by the pinned instance (CW03).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackageVersion {
    pub package: String,
    pub version: Option<String>,
    pub commit: Option<String>,
}

/// How one `widgets_values` position is consumed by a special rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialSlot {
    /// Position serializes to the named API input.
    Named(&'static str),
    /// Position is proven UI-only (e.g. `control_after_generate`); the string is the proof reason.
    UiOnly(&'static str),
    /// Position starts a dynamic group whose children are resolved by the named group input.
    DynamicGroup(&'static str),
    /// Position serializes to an API input a frontend extension creates (not in the definition);
    /// the value is passed through verbatim.
    FrontendInput(&'static str),
    /// Every remaining position serializes to `<prefix>1`, `<prefix>2`, ... (a frontend extension
    /// that grows one widget per saved value). Must be the last slot, after fixed slots only.
    FrontendSequence(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpecialRuleV1 {
    /// `"<class_type>@<package>@<version>"`.
    pub rule_id: &'static str,
    pub class_type: &'static str,
    pub package: &'static str,
    pub version: &'static str,
    /// Lowercase hex sha256 of the normalized class definition (CW30 `NodeDefinitionV1`).
    pub definition_digest: &'static str,
    pub widget_slots: &'static [SpecialSlot],
    /// Source reference: upstream file/commit and oracle fixture path.
    pub notes: &'static str,
}

/// Class still awaiting oracle evidence. Not a rule; never consulted by [`special_rule_for`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingSpecialClass {
    pub ledger_item: &'static str,
    pub class_type: &'static str,
    pub blocked_on: &'static [&'static str],
}

/// Offscreen bridge evidence (CW35): the class serializes through its pack's frontend extension.
const BRIDGE_EVIDENCE: &[&str] = &["CW35"];

/// CW38 U classes still without a proven capability. CW38.01-28 were exported by the official
/// frontend 1.53.10 (`tests/fixtures/comfy_import_v2/oracle/fe-1.53.10-cw38`); 27 are proven by
/// the generic serializer profile and left this list (fixtures `verified`, crate test
/// `oracle_cw38`). A class leaves this list only together with its fixture switching to
/// `verified` (see `special_fixture_consistency` test).
pub const PENDING_SPECIAL_CLASSES: &[PendingSpecialClass] = &[
    // The generic profile is refuted (KJNodes points-editor extension rewrites the values) and
    // the core refuses the class; it needs the bridge or a reviewed rule.
    PendingSpecialClass { ledger_item: "CW38.23", class_type: "PointsEditor", blocked_on: BRIDGE_EVIDENCE },
];

/// Reviewed rules. Empty until oracle evidence (CW03–CW05) exists for a class: nothing is registered
/// on the strength of a class name or of unpinned upstream source.
static RULES: &[SpecialRuleV1] = &[SpecialRuleV1 {
    rule_id: "CustomCombo@comfy_extras.nodes_logic@0.39.2",
    class_type: "CustomCombo",
    package: "comfy_extras.nodes_logic",
    version: "0.39.2",
    definition_digest: CUSTOM_COMBO_DIGEST_0_39_2,
    widget_slots: &[
        SpecialSlot::Named("choice"),
        SpecialSlot::FrontendInput("index"),
        SpecialSlot::FrontendSequence("option"),
    ],
    notes: "comfy-core CustomCombo frontend extension (frontend 1.53.10): widgets choice, hidden \
            index, option1..N grown per saved value, all serialized into the API prompt. Oracle: \
            src-tauri/tests/fixtures/comfy_import_v2/oracle/fe-1.53.10/Ideogram4__image_ideogram4_t2i.json \
            node 98:156; widget dump in 20261014-COMFY-parallel-lanes.md progress notes.",
}];

/// Normalized `CustomCombo` definition digest captured from ComfyUI 0.39.2 `/object_info`.
const CUSTOM_COMBO_DIGEST_0_39_2: &str = "d71f48c3f4f5a3a9e31f2b755069632de637ea43224a382b2f62707f45a97ed4";

pub fn all_rules() -> &'static [SpecialRuleV1] {
    RULES
}

/// Structured result of a rule lookup; every non-`Matched` variant is explicit missing evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecialRuleLookup {
    Matched(&'static SpecialRuleV1),
    /// No rule is registered for this class: the generic profile or the bridge must prove it.
    NoRuleForClass,
    /// Rules exist for the class, but for other packages.
    PackageMismatch { registered: Vec<&'static str> },
    /// The instance did not report a package version, so no rule can be proven applicable.
    VersionUnknown,
    /// Rules exist for the package, but for other versions.
    VersionMismatch { registered: Vec<&'static str> },
    /// Version matches but the live definition differs from the reviewed one.
    DefinitionDigestMismatch { expected: &'static str, actual: String },
}

impl SpecialRuleLookup {
    /// Stable diagnostic code for the capability resolver; `None` only for `Matched`/`NoRuleForClass`.
    pub fn missing_evidence_code(&self) -> Option<&'static str> {
        match self {
            SpecialRuleLookup::Matched(_) | SpecialRuleLookup::NoRuleForClass => None,
            SpecialRuleLookup::PackageMismatch { .. } => Some(codes::SPECIAL_RULE_PACKAGE_MISMATCH),
            SpecialRuleLookup::VersionUnknown => Some(codes::SPECIAL_RULE_VERSION_UNKNOWN),
            SpecialRuleLookup::VersionMismatch { .. } => Some(codes::SPECIAL_RULE_VERSION_MISMATCH),
            SpecialRuleLookup::DefinitionDigestMismatch { .. } => {
                Some(codes::SPECIAL_RULE_DEFINITION_DIGEST_MISMATCH)
            }
        }
    }
}

pub fn lookup_special_rule(
    class_type: &str,
    pkg: &PackageVersion,
    definition_digest: &str,
) -> SpecialRuleLookup {
    lookup_in(all_rules(), class_type, pkg, definition_digest)
}

pub fn special_rule_for(
    class_type: &str,
    pkg: &PackageVersion,
    definition_digest: &str,
) -> Option<&'static SpecialRuleV1> {
    match lookup_special_rule(class_type, pkg, definition_digest) {
        SpecialRuleLookup::Matched(rule) => Some(rule),
        _ => None,
    }
}

fn lookup_in(
    rules: &'static [SpecialRuleV1],
    class_type: &str,
    pkg: &PackageVersion,
    definition_digest: &str,
) -> SpecialRuleLookup {
    let for_class: Vec<&'static SpecialRuleV1> =
        rules.iter().filter(|r| r.class_type == class_type).collect();
    if for_class.is_empty() {
        return SpecialRuleLookup::NoRuleForClass;
    }
    let for_package: Vec<&'static SpecialRuleV1> =
        for_class.iter().copied().filter(|r| r.package == pkg.package).collect();
    if for_package.is_empty() {
        return SpecialRuleLookup::PackageMismatch {
            registered: for_class.iter().map(|r| r.package).collect(),
        };
    }
    let Some(version) = pkg.version.as_deref() else {
        return SpecialRuleLookup::VersionUnknown;
    };
    let Some(rule) = for_package.iter().copied().find(|r| r.version == version) else {
        return SpecialRuleLookup::VersionMismatch {
            registered: for_package.iter().map(|r| r.version).collect(),
        };
    };
    // Hosts sign definitions as `sha256:<hex>`; rules pin the bare hex.
    let actual_hex = definition_digest.strip_prefix("sha256:").unwrap_or(definition_digest);
    if rule.definition_digest != actual_hex {
        return SpecialRuleLookup::DefinitionDigestMismatch {
            expected: rule.definition_digest,
            actual: definition_digest.to_string(),
        };
    }
    SpecialRuleLookup::Matched(rule)
}

/// Structural problems in a rule table; the shipped table must produce none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleRegistryError {
    DuplicateRuleId(&'static str),
    DuplicateTuple { class_type: &'static str, package: &'static str, version: &'static str },
    RuleIdShape(&'static str),
    EmptyField { rule_id: &'static str, field: &'static str },
    DigestShape(&'static str),
    NoSlots(&'static str),
    DuplicateSlotName { rule_id: &'static str, name: &'static str },
    StillPending(&'static str),
}

pub fn validate_registry() -> Vec<RuleRegistryError> {
    validate_table(all_rules(), PENDING_SPECIAL_CLASSES)
}

fn validate_table(
    rules: &[SpecialRuleV1],
    pending: &[PendingSpecialClass],
) -> Vec<RuleRegistryError> {
    let mut errors = Vec::new();
    for (i, rule) in rules.iter().enumerate() {
        for (field, value) in [
            ("class_type", rule.class_type),
            ("package", rule.package),
            ("version", rule.version),
            ("notes", rule.notes),
        ] {
            if value.trim().is_empty() {
                errors.push(RuleRegistryError::EmptyField { rule_id: rule.rule_id, field });
            }
        }
        let expected_id = format!("{}@{}@{}", rule.class_type, rule.package, rule.version);
        if rule.rule_id != expected_id {
            errors.push(RuleRegistryError::RuleIdShape(rule.rule_id));
        }
        let digest = rule.definition_digest;
        if digest.len() != 64 || !digest.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            errors.push(RuleRegistryError::DigestShape(rule.rule_id));
        }
        if rule.widget_slots.is_empty() {
            errors.push(RuleRegistryError::NoSlots(rule.rule_id));
        }
        let mut seen = alloc::collections::BTreeSet::new();
        for slot in rule.widget_slots {
            if let SpecialSlot::Named(name) | SpecialSlot::DynamicGroup(name) | SpecialSlot::FrontendInput(name) = slot {
                if !seen.insert(*name) {
                    errors.push(RuleRegistryError::DuplicateSlotName { rule_id: rule.rule_id, name });
                }
            }
        }
        for other in &rules[..i] {
            if other.rule_id == rule.rule_id {
                errors.push(RuleRegistryError::DuplicateRuleId(rule.rule_id));
            }
            if other.class_type == rule.class_type
                && other.package == rule.package
                && other.version == rule.version
            {
                errors.push(RuleRegistryError::DuplicateTuple {
                    class_type: rule.class_type,
                    package: rule.package,
                    version: rule.version,
                });
            }
        }
        if pending.iter().any(|p| p.class_type == rule.class_type) {
            errors.push(RuleRegistryError::StillPending(rule.rule_id));
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    // Synthetic table for lookup semantics only; class/package names are not real U classes.
    static SYNTHETIC: &[SpecialRuleV1] = &[
        SpecialRuleV1 {
            rule_id: "SyntheticNode@pkg-x@1.0.0",
            class_type: "SyntheticNode",
            package: "pkg-x",
            version: "1.0.0",
            definition_digest: DIGEST_A,
            widget_slots: &[SpecialSlot::Named("a"), SpecialSlot::UiOnly("control_after_generate")],
            notes: "synthetic",
        },
        SpecialRuleV1 {
            rule_id: "SyntheticNode@pkg-x@2.0.0",
            class_type: "SyntheticNode",
            package: "pkg-x",
            version: "2.0.0",
            definition_digest: DIGEST_B,
            widget_slots: &[SpecialSlot::Named("a"), SpecialSlot::Named("b")],
            notes: "synthetic",
        },
    ];

    fn pkg(package: &str, version: Option<&str>) -> PackageVersion {
        PackageVersion {
            package: package.to_string(),
            version: version.map(str::to_string),
            commit: None,
        }
    }

    #[test]
    fn same_class_different_versions_are_isolated() {
        let v1 = lookup_in(SYNTHETIC, "SyntheticNode", &pkg("pkg-x", Some("1.0.0")), DIGEST_A);
        assert_eq!(v1, SpecialRuleLookup::Matched(&SYNTHETIC[0]));
        let v2 = lookup_in(SYNTHETIC, "SyntheticNode", &pkg("pkg-x", Some("2.0.0")), DIGEST_B);
        assert_eq!(v2, SpecialRuleLookup::Matched(&SYNTHETIC[1]));
        // v1 digest with v2 version must not borrow the v2 rule.
        let crossed = lookup_in(SYNTHETIC, "SyntheticNode", &pkg("pkg-x", Some("2.0.0")), DIGEST_A);
        assert_eq!(
            crossed,
            SpecialRuleLookup::DefinitionDigestMismatch { expected: DIGEST_B, actual: DIGEST_A.to_string() }
        );
    }

    #[test]
    fn version_mismatch_and_unknown_are_explicit_missing_evidence() {
        let mismatch = lookup_in(SYNTHETIC, "SyntheticNode", &pkg("pkg-x", Some("1.0.1")), DIGEST_A);
        assert_eq!(mismatch, SpecialRuleLookup::VersionMismatch { registered: vec!["1.0.0", "2.0.0"] });
        assert_eq!(mismatch.missing_evidence_code(), Some("workflowSpecialRuleVersionMismatch"));
        let unknown = lookup_in(SYNTHETIC, "SyntheticNode", &pkg("pkg-x", None), DIGEST_A);
        assert_eq!(unknown, SpecialRuleLookup::VersionUnknown);
        assert_eq!(unknown.missing_evidence_code(), Some("workflowSpecialRuleVersionUnknown"));
        let other_pkg = lookup_in(SYNTHETIC, "SyntheticNode", &pkg("pkg-y", Some("1.0.0")), DIGEST_A);
        assert_eq!(other_pkg, SpecialRuleLookup::PackageMismatch { registered: vec!["pkg-x", "pkg-x"] });
        assert_eq!(other_pkg.missing_evidence_code(), Some("workflowSpecialRulePackageMismatch"));
    }

    #[test]
    fn class_name_alone_never_selects_a_rule() {
        let none = lookup_in(SYNTHETIC, "OtherNode", &pkg("pkg-x", Some("1.0.0")), DIGEST_A);
        assert_eq!(none, SpecialRuleLookup::NoRuleForClass);
        assert_eq!(none.missing_evidence_code(), None);
        for class in PENDING_SPECIAL_CLASSES {
            for rule in SYNTHETIC {
                let p = pkg(rule.package, Some(rule.version));
                assert_eq!(special_rule_for(class.class_type, &p, rule.definition_digest), None);
            }
        }
    }

    #[test]
    fn shipped_registry_is_structurally_valid() {
        assert_eq!(validate_registry(), Vec::<RuleRegistryError>::new());
        assert_eq!(validate_table(SYNTHETIC, PENDING_SPECIAL_CLASSES), Vec::<RuleRegistryError>::new());
    }

    #[test]
    fn registry_validation_rejects_bad_rules() {
        static BAD: &[SpecialRuleV1] = &[
            SpecialRuleV1 {
                rule_id: "wrong-id",
                class_type: "ImageScale",
                package: "p",
                version: "1",
                definition_digest: "XYZ",
                widget_slots: &[SpecialSlot::Named("a"), SpecialSlot::Named("a")],
                notes: "n",
            },
            SpecialRuleV1 {
                rule_id: "wrong-id",
                class_type: "ImageScale",
                package: "p",
                version: "1",
                definition_digest: DIGEST_A,
                widget_slots: &[],
                notes: " ",
            },
        ];
        const PENDING: &[PendingSpecialClass] =
            &[PendingSpecialClass { ledger_item: "CW38.11", class_type: "ImageScale", blocked_on: &["CW03"] }];
        let errors = validate_table(BAD, PENDING);
        assert_eq!(
            errors,
            vec![
                RuleRegistryError::RuleIdShape("wrong-id"),
                RuleRegistryError::DigestShape("wrong-id"),
                RuleRegistryError::DuplicateSlotName { rule_id: "wrong-id", name: "a" },
                RuleRegistryError::StillPending("wrong-id"),
                RuleRegistryError::EmptyField { rule_id: "wrong-id", field: "notes" },
                RuleRegistryError::RuleIdShape("wrong-id"),
                RuleRegistryError::NoSlots("wrong-id"),
                RuleRegistryError::DuplicateRuleId("wrong-id"),
                RuleRegistryError::DuplicateTuple { class_type: "ImageScale", package: "p", version: "1" },
                RuleRegistryError::StillPending("wrong-id"),
            ]
        );
    }

    #[test]
    fn package_version_dto_is_strict_camel_case() {
        let ok: PackageVersion =
            serde_json::from_str(r#"{"package":"comfy-core","version":"0.3.0","commit":null}"#)
                .expect("valid PackageVersion");
        assert_eq!(ok, pkg("comfy-core", Some("0.3.0")));
        let unknown_field =
            serde_json::from_str::<PackageVersion>(r#"{"package":"p","version":null,"commit":null,"x":1}"#);
        assert!(unknown_field.is_err());
    }

    macro_rules! fixtures {
        ($($name:literal),* $(,)?) => {
            &[$(($name, include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/widgets/",
                $name,
                ".json"
            )))),*]
        };
    }

    const FIXTURES: &[(&str, &str)] = fixtures![
        "BlockSparseAttention", "BlockifyMask", "CLIPVisionEncode", "CLIPVisionLoader",
        "DWPreprocessor", "DownloadAndLoadSAM2Model", "DrawMaskOnImage", "EmptyLTXVLatentVideo",
        "GrowMask", "ImageFromBatch", "ImageScale", "LTXVConditioning", "LTXVDualCFGGuider",
        "LTXVEmptyLatentAudio", "LTXVImgToVideoInplace", "LTXVPreprocess", "LatentUpscaleModelLoader",
        "ManualSigmas", "MiniMaxH3SigmaShift", "ModelAttentionBackend", "ModelSamplingSD3",
        "PixelPerfectResolution", "PointsEditor", "ResizeImageMaskNode", "Sam2Segmentation",
        "TrimVideoLatent", "VAEDecodeTiled", "WanAnimateToVideo",
    ];

    fn field<'a>(v: &'a Value, name: &str, file: &str) -> &'a Value {
        v.get(name).unwrap_or_else(|| panic!("{file}: missing field {name}"))
    }

    fn str_field<'a>(v: &'a Value, name: &str, file: &str) -> &'a str {
        field(v, name, file).as_str().unwrap_or_else(|| panic!("{file}: {name} must be a string"))
    }

    #[test]
    fn special_fixture_consistency() {
        assert_eq!(FIXTURES.len(), 28);
        let mut ledger_items = Vec::new();
        for (index, (name, text)) in FIXTURES.iter().enumerate() {
            let fx: Value = serde_json::from_str(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(field(&fx, "fixtureVersion", name), &Value::from(1u64), "{name}");
            assert_eq!(
                field(&fx, "ruleRegistryVersion", name),
                &Value::from(u64::from(RULE_REGISTRY_VERSION)),
                "{name}"
            );
            assert_eq!(str_field(&fx, "classType", name), *name);
            let ledger = str_field(&fx, "ledgerItem", name);
            assert_eq!(ledger, format!("CW38.{:02}", index + 1), "{name}");
            ledger_items.push(ledger.to_string());

            let rules_for_class: Vec<&SpecialRuleV1> =
                all_rules().iter().filter(|r| r.class_type == *name).collect();
            let pending = PENDING_SPECIAL_CLASSES.iter().find(|p| p.class_type == *name);
            let evidence = field(&fx, "evidence", name);
            let positives = field(&fx, "positiveCases", name)
                .as_array()
                .unwrap_or_else(|| panic!("{name}: positiveCases must be an array"));
            let negatives = field(&fx, "negativeCases", name)
                .as_array()
                .unwrap_or_else(|| panic!("{name}: negativeCases must be an array"));

            let mut kinds: Vec<&str> = negatives.iter().map(|c| str_field(c, "kind", name)).collect();
            kinds.sort_unstable();
            assert_eq!(kinds, vec!["badEnum", "leftoverWidget", "versionMismatch"], "{name}");

            match str_field(&fx, "status", name) {
                "blocked" => {
                    let pending = pending.unwrap_or_else(|| panic!("{name}: blocked but not pending"));
                    assert_eq!(pending.ledger_item, ledger);
                    let blocked_on: Vec<&str> = field(&fx, "blockedOn", name)
                        .as_array()
                        .unwrap_or_else(|| panic!("{name}: blockedOn must be an array"))
                        .iter()
                        .map(|v| v.as_str().unwrap_or_else(|| panic!("{name}: blockedOn entry")))
                        .collect();
                    assert_eq!(blocked_on, pending.blocked_on.to_vec(), "{name}");
                    assert!(rules_for_class.is_empty(), "{name}: rule registered without evidence");
                    assert!(positives.is_empty(), "{name}: positive case without oracle");
                    str_field(&fx, "blockedReason", name);
                    match str_field(&fx, "genericProfileCheck", name) {
                        "pending" => {
                            for key in ["package", "version", "definitionDigest", "oracleApiRef"] {
                                assert_eq!(field(evidence, key, name), &Value::Null, "{name}: {key}");
                            }
                            for case in negatives {
                                assert_eq!(str_field(case, "status", name), "pendingOracle", "{name}");
                            }
                        }
                        // Oracle exists and shows the generic profile cannot reproduce the class:
                        // every case must be refused with an explicit code.
                        "genericProfileRefuted" => {
                            for key in ["package", "version", "definitionDigest", "oracleApiRef"] {
                                str_field(evidence, key, name);
                            }
                            let refuting = field(&fx, "refutingCases", name)
                                .as_array()
                                .unwrap_or_else(|| panic!("{name}: refutingCases must be an array"));
                            assert!(!refuting.is_empty(), "{name}: refuted without oracle cases");
                            for case in refuting.iter().chain(negatives) {
                                str_field(case, "expectedCode", name);
                            }
                            for case in negatives {
                                assert_eq!(str_field(case, "status", name), "active", "{name}");
                            }
                        }
                        other => panic!("{name}: unknown blocked genericProfileCheck {other}"),
                    }
                }
                "verified" => {
                    assert!(pending.is_none(), "{name}: verified but still pending");
                    assert!(!positives.is_empty(), "{name}: verified without positive cases");
                    let package = str_field(evidence, "package", name);
                    let version = str_field(evidence, "version", name);
                    let digest = str_field(evidence, "definitionDigest", name);
                    str_field(evidence, "oracleApiRef", name);
                    match str_field(&fx, "genericProfileCheck", name) {
                        "genericProfileProven" => {
                            assert!(rules_for_class.is_empty(), "{name}: generic class has a rule");
                        }
                        "specialRuleRequired" => {
                            let p = pkg(package, Some(version));
                            let rule = special_rule_for(name, &p, digest)
                                .unwrap_or_else(|| panic!("{name}: evidence tuple has no rule"));
                            let mismatched = pkg(package, Some(&format!("{version}+mismatch")));
                            assert!(special_rule_for(name, &mismatched, digest).is_none(), "{name}");
                            assert_eq!(
                                lookup_special_rule(name, &mismatched, digest).missing_evidence_code(),
                                Some("workflowSpecialRuleVersionMismatch")
                            );
                            assert_eq!(rule.definition_digest, digest);
                        }
                        other => panic!("{name}: unknown genericProfileCheck {other}"),
                    }
                    for case in positives {
                        assert_eq!(str_field(case, "expectedOutcome", name), "matchesOracle", "{name}");
                        str_field(case, "api", name);
                    }
                    for case in negatives {
                        match str_field(case, "status", name) {
                            // Either an explicit stop code, or the official export shows the
                            // frontend accepts the value and the core must reproduce that export.
                            "active" => match (case.get("expectedCode"), case.get("expectedOutcome")) {
                                (Some(Value::String(_)), None) => {}
                                (None, Some(Value::String(o))) if o == "matchesOracle" => {
                                    str_field(case, "api", name);
                                }
                                _ => panic!("{name}: active case needs expectedCode or matchesOracle + api"),
                            },
                            "notApplicable" => {
                                assert_eq!(str_field(case, "kind", name), "badEnum", "{name}");
                                str_field(case, "reason", name);
                            }
                            other => panic!("{name}: unknown case status {other}"),
                        }
                    }
                }
                other => panic!("{name}: unknown status {other}"),
            }
        }
        let pending_items: Vec<&str> = PENDING_SPECIAL_CLASSES.iter().map(|p| p.ledger_item).collect();
        for item in pending_items {
            assert!(ledger_items.iter().any(|l| l == item), "{item} has no fixture");
        }
    }
}

/// Closed import diagnostic codes for special-rule evidence gaps (comfy-workflow-import-v1 diagnosticCode).
pub mod codes {
    pub const SPECIAL_RULE_PACKAGE_MISMATCH: &str = "workflowSpecialRulePackageMismatch";
    pub const SPECIAL_RULE_VERSION_UNKNOWN: &str = "workflowSpecialRuleVersionUnknown";
    pub const SPECIAL_RULE_VERSION_MISMATCH: &str = "workflowSpecialRuleVersionMismatch";
    pub const SPECIAL_RULE_DEFINITION_DIGEST_MISMATCH: &str = "workflowSpecialRuleDefinitionDigestMismatch";
}

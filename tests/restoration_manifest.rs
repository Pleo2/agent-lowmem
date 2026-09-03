use serde_json::Value;
use std::{fs, path::Path};

#[test]
fn schema_is_closed_and_models_only_one_prior_applied_manifest() {
    let schema = schema();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["$ref"], "#/$defs/manifest");
    assert_eq!(
        schema.pointer("/$defs/manifest/unevaluatedProperties"),
        Some(&Value::Bool(false))
    );
    assert_eq!(
        schema.pointer("/$defs/priorApplied/unevaluatedProperties"),
        Some(&Value::Bool(false))
    );
    assert!(
        schema
            .pointer("/$defs/priorApplied/properties/previousApplied")
            .is_none()
    );
    assert_eq!(
        schema.pointer("/$defs/priorApplied/allOf/1/properties/state/const"),
        Some(&Value::String("applied".into()))
    );
}

#[test]
fn schema_closes_nested_objects_and_bounds_hashes_modes_spans_and_separators() {
    let schema = schema();
    for pointer in [
        "/$defs/ownedBytes/additionalProperties",
        "/$defs/priorManagedState/oneOf/0/additionalProperties",
        "/$defs/priorManagedState/oneOf/1/additionalProperties",
        "/$defs/configuration/additionalProperties",
        "/$defs/span/additionalProperties",
        "/$defs/agentsPolicy/additionalProperties",
    ] {
        assert_eq!(
            schema.pointer(pointer),
            Some(&Value::Bool(false)),
            "{pointer}"
        );
    }
    assert_eq!(
        schema
            .pointer("/$defs/sha256/pattern")
            .and_then(Value::as_str),
        Some("^[0-9a-f]{64}$")
    );
    assert_eq!(
        schema
            .pointer("/$defs/mode/maximum")
            .and_then(Value::as_u64),
        Some(438)
    );
    assert_eq!(
        schema
            .pointer("/$defs/span/properties/end/maximum")
            .and_then(Value::as_u64),
        Some(1_048_576)
    );
    assert_eq!(
        schema
            .pointer("/$defs/agentsPolicy/properties/insertedSeparator/enum")
            .unwrap(),
        &serde_json::json!(["", "\n", "\n\n"])
    );
}

#[test]
fn schema_has_no_field_for_paths_users_environment_or_external_bytes() {
    let serialized = serde_json::to_string(&schema()).unwrap();
    for forbidden in [
        "absolutePath",
        "repositoryPath",
        "username",
        "environment",
        "externalBytes",
        "prefixBytes",
        "suffixBytes",
        "timestamp",
    ] {
        assert!(!serialized.contains(forbidden));
    }
}

fn schema() -> Value {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas/restoration-manifest-v1.schema.json");
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

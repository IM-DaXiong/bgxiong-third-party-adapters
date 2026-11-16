#[cfg(target_arch = "wasm32")]
use alloc::{string::String, vec::Vec};
use crate::error::{MappingError, MappingErrorCode};
use crate::types::WorkflowIdentityV1;

fn valid_token(value: &str) -> bool {
    !value.trim().is_empty()
        && !value.chars().any(|c| c.is_control())
        && !value.contains('?')
        && !value.contains('#')
}

pub fn normalize_source_path(raw: &str) -> Result<String, MappingError> {
    let replaced = raw.trim().replace('\\', "/");
    if replaced.is_empty() || replaced.starts_with('/') || replaced.contains(':') {
        return Err(MappingError::new(
            MappingErrorCode::IdentityInvalid,
            "sourcePath must be relative",
        ));
    }
    let mut out = Vec::new();
    for segment in replaced.split('/') {
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.chars().any(|c| c.is_control())
        {
            return Err(MappingError::new(
                MappingErrorCode::IdentityInvalid,
                "sourcePath contains an unsafe segment",
            ));
        }
        out.push(segment);
    }
    Ok(out.join("/"))
}

pub fn validate_identity(identity: &WorkflowIdentityV1) -> Result<(), MappingError> {
    if identity.schema_version != 1
        || !valid_token(&identity.instance_id)
        || !valid_token(&identity.user_scope)
    {
        return Err(MappingError::new(
            MappingErrorCode::IdentityInvalid,
            "invalid identity version or scope",
        ));
    }
    normalize_source_path(&identity.source_path)?;
    if identity.display_name.trim().is_empty()
        || identity.display_name.chars().any(|c| c.is_control())
    {
        return Err(MappingError::new(
            MappingErrorCode::IdentityInvalid,
            "displayName is invalid",
        ));
    }
    Ok(())
}

pub fn stable_workflow_key(identity: &WorkflowIdentityV1) -> Result<String, MappingError> {
    validate_identity(identity)?;
    Ok(format!(
        "{}|{}|{:?}|{}",
        identity.instance_id,
        identity.user_scope,
        identity.source_kind,
        normalize_source_path(&identity.source_path)?
    ))
}

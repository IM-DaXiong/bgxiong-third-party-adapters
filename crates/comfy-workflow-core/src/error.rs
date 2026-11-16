#[cfg(target_arch = "wasm32")]
use alloc::{string::String};
use core::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MappingErrorCode {
    IdentityInvalid,
    GraphInvalid,
    OutputAmbiguous,
    BindingAmbiguous,
    TargetMissing,
    TypeMismatch,
    ValueOutOfRange,
    WriteConflict,
    ContractStale,
    NodeDefinitionIncomplete,
    ResourceLimitExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MappingError {
    pub code: MappingErrorCode,
    pub detail: String,
}

impl MappingError {
    pub fn new(code: MappingErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for MappingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.detail)
    }
}

impl core::error::Error for MappingError {}

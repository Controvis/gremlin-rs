use crate::{GremlinError, GremlinResult};
use crate::structure::{GValue, List};
use serde_json::Value;

pub fn deserializer_cosmos(value: &Value) -> GremlinResult<GValue> {
    // Handle empty list responses
    if let Value::Array(arr) = value {
        if arr.is_empty() {
            return Ok(GValue::List(List::new(vec![])));
        }
    }

    Err(GremlinError::Generic(format!(
        "Cosmos deserialization not yet implemented. Response contents: {}",
        serde_json::to_string_pretty(value).unwrap_or_else(|_| format!("{:?}", value))
    )))
}

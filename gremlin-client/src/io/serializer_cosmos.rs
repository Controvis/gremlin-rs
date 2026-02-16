use crate::{GremlinError, GremlinResult};
use crate::structure::{GValue, List, Path, Vertex, Edge, Property, GID, Map};
use serde_json::Value;
use std::collections::HashMap;

pub fn deserializer_cosmos(value: &Value) -> GremlinResult<GValue> {
    if let Value::Array(arr) = value {
        if arr.is_empty() {
            return Ok(GValue::List(List::new(vec![])));
        }

        let mut paths = Vec::new();

        for path_item in arr {
            let path_obj = path_item.as_object()
                .ok_or_else(|| GremlinError::Generic("Expected path object".to_string()))?;

            let labels_value = path_obj.get("labels");
            let labels = if let Some(lv) = labels_value {
                deserialize_labels(lv)?
            } else {
                GValue::List(List::new(vec![]))
            };

            let objects_value = path_obj.get("objects");
            let objects = if let Some(lv) = objects_value {
                deserialize_objects(lv)?
            } else {
                List::new(vec![])
            };

            paths.push(GValue::Path(Path::new(labels, objects)));
        }

        return Ok(GValue::List(List::new(paths)));
    }

    Err(GremlinError::Generic(format!(
        "Expected array response from Cosmos. Got: {}",
        serde_json::to_string_pretty(value).unwrap_or_else(|_| format!("{:?}", value))
    )))
}

fn deserialize_labels(value: &Value) -> GremlinResult<GValue> {
    let labels_arr = value.as_array()
        .ok_or_else(|| GremlinError::Generic("Expected labels array".to_string()))?;

    let mut label_lists = Vec::new();
    for label_item in labels_arr {
        let label_arr = label_item.as_array()
            .ok_or_else(|| GremlinError::Generic("Expected label item to be array".to_string()))?;

        let labels: Vec<GValue> = label_arr.iter()
            .map(|v| v.as_str()
                .map(|s| GValue::String(s.to_string()))
                .unwrap_or(GValue::Null))
            .collect();

        label_lists.push(GValue::List(List::new(labels)));
    }

    Ok(GValue::List(List::new(label_lists)))
}

fn deserialize_objects(value: &Value) -> GremlinResult<List> {
    let objects_arr = value.as_array()
        .ok_or_else(|| GremlinError::Generic("Expected objects array".to_string()))?;

    let mut result = Vec::new();
    let mut i = 0;

    while i < objects_arr.len() {
        let obj = objects_arr[i].as_object()
            .ok_or_else(|| GremlinError::Generic("Expected object".to_string()))?;

        let label = obj.get("label")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GremlinError::Generic("Missing 'label' field".to_string()))?;

        // Check if this is a vertex (node) or edge (connection)
        if label == "node" || !obj.contains_key("relationship") {
            // It's a vertex
            let vertex = deserialize_vertex(obj)?;
            result.push(GValue::Vertex(vertex));
            i += 1;
        } else {
            // It's an edge - we need the previous and next vertices
            if i == 0 || i >= objects_arr.len() - 1 {
                return Err(GremlinError::Generic(
                    "Edge must be between two vertices".to_string()
                ));
            }

            let out_v_obj = objects_arr[i - 1].as_object()
                .ok_or_else(|| GremlinError::Generic("Expected out vertex object".to_string()))?;
            let in_v_obj = objects_arr[i + 1].as_object()
                .ok_or_else(|| GremlinError::Generic("Expected in vertex object".to_string()))?;

            let edge = deserialize_edge(obj, out_v_obj, in_v_obj)?;
            result.push(GValue::Edge(edge));
            i += 1;
        }
    }

    Ok(List::new(result))
}

fn deserialize_vertex(obj: &serde_json::Map<String, Value>) -> GremlinResult<Vertex> {
    let id = obj.get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GremlinError::Generic("Missing 'id' field".to_string()))?;

    let label = obj.get("label")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GremlinError::Generic("Missing 'label' field".to_string()))?;

    // Create properties map from remaining fields
    let properties = HashMap::new();

    Ok(Vertex::new(
        GID::String(id.to_string()),
        label,
        properties,
    ))
}

fn deserialize_edge(
    obj: &serde_json::Map<String, Value>,
    out_v_obj: &serde_json::Map<String, Value>,
    in_v_obj: &serde_json::Map<String, Value>,
) -> GremlinResult<Edge> {
    let id = obj.get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GremlinError::Generic("Missing edge 'id' field".to_string()))?;

    let label = obj.get("label")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GremlinError::Generic("Missing edge 'label' field".to_string()))?;

    let out_v_id = out_v_obj.get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GremlinError::Generic("Missing out vertex 'id'".to_string()))?;

    let out_v_label = out_v_obj.get("label")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GremlinError::Generic("Missing out vertex 'label'".to_string()))?;

    let in_v_id = in_v_obj.get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GremlinError::Generic("Missing in vertex 'id'".to_string()))?;

    let in_v_label = in_v_obj.get("label")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GremlinError::Generic("Missing in vertex 'label'".to_string()))?;

    // Create properties map from edge fields (relationship, weight, etc.)
    let mut properties = HashMap::new();

    if let Some(relationship) = obj.get("relationship").and_then(|v| v.as_str()) {
        properties.insert(
            "relationship".to_string(),
            Property::new("relationship", GValue::String(relationship.to_string()))
        );
    }

    if let Some(weight) = obj.get("weight").and_then(|v| v.as_f64()) {
        properties.insert(
            "weight".to_string(),
            Property::new("weight", GValue::Double(weight))
        );
    }

    if let Some(namespace) = obj.get("namespace").and_then(|v| v.as_str()) {
        properties.insert(
            "namespace".to_string(),
            Property::new("namespace", GValue::String(namespace.to_string()))
        );
    }

    Ok(Edge::new(
        GID::String(id.to_string()),
        label,
        GID::String(in_v_id.to_string()),
        in_v_label,
        GID::String(out_v_id.to_string()),
        out_v_label,
        properties,
    ))
}

use crate::{GremlinError, GremlinResult};
use crate::structure::{GValue, List, Path, Vertex, Edge, Property, GID, Map, VertexProperty};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, PartialEq)]
enum CosmosResponseType {
    Path,
    Object,
    Primitive,
    Map,
}

pub fn deserializer_cosmos(value: &Value) -> GremlinResult<GValue> {
    if let Value::Array(arr) = value {
        if arr.is_empty() {
            return Ok(GValue::List(List::new(vec![])));
        }

        // Detect the response type by examining the first element
        let response_type = detect_cosmos_response_type(arr)?;

        return match response_type {
            CosmosResponseType::Path => deserialize_path_type(arr),
            CosmosResponseType::Object => deserialize_object_type(arr),
            CosmosResponseType::Primitive => deserialize_primitive_type(arr),
            CosmosResponseType::Map => deserialize_map_type(arr),
        };
    }

    Err(GremlinError::Generic(format!(
        "Expected array response from Cosmos. Got: {}",
        serde_json::to_string_pretty(value).unwrap_or_else(|_| format!("{:?}", value))
    )))
}

fn detect_cosmos_response_type(arr: &[Value]) -> GremlinResult<CosmosResponseType> {
    let first_item = arr.first()
        .ok_or_else(|| GremlinError::Generic("Empty array".to_string()))?;

    // Check if it's a string (primitive type)
    if first_item.is_string() {
        return Ok(CosmosResponseType::Primitive);
    }

    let first_obj = first_item.as_object()
        .ok_or_else(|| GremlinError::Generic("Expected object in array".to_string()))?;

    // Check if it's a path type (has "labels" and "objects" fields)
    if first_obj.contains_key("labels") && first_obj.contains_key("objects") {
        return Ok(CosmosResponseType::Path);
    }

    // Check if it's an object type (has "type" field with "vertex" or "edge")
    if first_obj.contains_key("type") {
        if let Some(type_str) = first_obj.get("type").and_then(|v| v.as_str()) {
            if type_str == "vertex" || type_str == "edge" {
                return Ok(CosmosResponseType::Object);
            }
        }
    }

    // Check if it's a map type (object with nested vertex/edge objects)
    // The first element's values should be objects with "type" field
    for (_, value) in first_obj {
        if let Some(nested_obj) = value.as_object() {
            if nested_obj.contains_key("type") {
                if let Some(type_str) = nested_obj.get("type").and_then(|v| v.as_str()) {
                    if type_str == "vertex" || type_str == "edge" {
                        return Ok(CosmosResponseType::Map);
                    }
                }
            }
        }
    }

    Err(GremlinError::Generic(format!(
        "Unable to determine Cosmos response type. First element: {}",
        serde_json::to_string_pretty(first_item).unwrap_or_else(|_| format!("{:?}", first_item))
    )))
}

fn deserialize_primitive_type(arr: &[Value]) -> GremlinResult<GValue> {
    let results: Vec<GValue> = arr.iter()
        .map(json_to_gvalue)
        .collect();

    Ok(GValue::List(List::new(results)))
}

fn deserialize_map_type(arr: &[Value]) -> GremlinResult<GValue> {
    let mut results = Vec::new();

    for item in arr {
        let obj = item.as_object()
            .ok_or_else(|| GremlinError::Generic("Expected object in map array".to_string()))?;

        let mut map = HashMap::new();

        for (key, value) in obj {
            // Each value might be a vertex, edge, or other value
            if let Some(nested_obj) = value.as_object() {
                if let Some(type_str) = nested_obj.get("type").and_then(|v| v.as_str()) {
                    match type_str {
                        "vertex" => {
                            let vertex = deserialize_vertex_with_properties(nested_obj)?;
                            map.insert(key.clone(), GValue::Vertex(vertex));
                        }
                        "edge" => {
                            let edge = deserialize_edge_standalone(nested_obj)?;
                            map.insert(key.clone(), GValue::Edge(edge));
                        }
                        _ => {
                            map.insert(key.clone(), json_to_gvalue(value));
                        }
                    }
                } else {
                    map.insert(key.clone(), json_to_gvalue(value));
                }
            } else {
                map.insert(key.clone(), json_to_gvalue(value));
            }
        }

        results.push(GValue::Map(Map::from(map)));
    }

    Ok(GValue::List(List::new(results)))
}

fn deserialize_path_type(arr: &[Value]) -> GremlinResult<GValue> {
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

    Ok(GValue::List(List::new(paths)))
}

fn deserialize_object_type(arr: &[Value]) -> GremlinResult<GValue> {
    let mut results = Vec::new();

    for item in arr {
        let obj = item.as_object()
            .ok_or_else(|| GremlinError::Generic("Expected object".to_string()))?;

        let item_type = obj.get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GremlinError::Generic("Missing 'type' field".to_string()))?;

        match item_type {
            "vertex" => {
                let vertex = deserialize_vertex_with_properties(obj)?;
                results.push(GValue::Vertex(vertex));
            }
            "edge" => {
                let edge = deserialize_edge_standalone(obj)?;
                results.push(GValue::Edge(edge));
            }
            _ => {
                return Err(GremlinError::Generic(format!("Unknown type: {}", item_type)));
            }
        }
    }

    Ok(GValue::List(List::new(results)))
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

fn deserialize_vertex_with_properties(obj: &serde_json::Map<String, Value>) -> GremlinResult<Vertex> {
    let id = obj.get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GremlinError::Generic("Missing 'id' field".to_string()))?;

    let label = obj.get("label")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GremlinError::Generic("Missing 'label' field".to_string()))?;

    // Parse properties from the "properties" field
    let mut properties: HashMap<String, Vec<VertexProperty>> = HashMap::new();

    if let Some(props_value) = obj.get("properties") {
        if let Some(props_obj) = props_value.as_object() {
            for (prop_name, prop_value) in props_obj {
                // Each property is an array of property objects with id and value
                if let Some(prop_array) = prop_value.as_array() {
                    let mut vertex_props = Vec::new();
                    for prop_item in prop_array {
                        if let Some(prop_item_obj) = prop_item.as_object() {
                            if let Some(value) = prop_item_obj.get("value") {
                                let prop_id = prop_item_obj.get("id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let gvalue = json_to_gvalue(value);
                                vertex_props.push(VertexProperty::new(
                                    GID::String(prop_id.to_string()),
                                    prop_name,
                                    gvalue
                                ));
                            }
                        }
                    }
                    if !vertex_props.is_empty() {
                        properties.insert(prop_name.clone(), vertex_props);
                    }
                }
            }
        }
    }

    Ok(Vertex::new(
        GID::String(id.to_string()),
        label,
        properties,
    ))
}

fn deserialize_edge_standalone(obj: &serde_json::Map<String, Value>) -> GremlinResult<Edge> {
    let id = obj.get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GremlinError::Generic("Missing edge 'id' field".to_string()))?;

    let label = obj.get("label")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GremlinError::Generic("Missing edge 'label' field".to_string()))?;

    // For standalone edges, we need to extract inV and outV information
    let in_v = obj.get("inV")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GremlinError::Generic("Missing edge 'inV' field".to_string()))?;

    let in_v_label = obj.get("inVLabel")
        .and_then(|v| v.as_str())
        .unwrap_or("vertex");

    let out_v = obj.get("outV")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GremlinError::Generic("Missing edge 'outV' field".to_string()))?;

    let out_v_label = obj.get("outVLabel")
        .and_then(|v| v.as_str())
        .unwrap_or("vertex");

    // Parse properties from the "properties" field if present
    let mut properties = HashMap::new();

    if let Some(props_value) = obj.get("properties") {
        if let Some(props_obj) = props_value.as_object() {
            for (prop_name, prop_value) in props_obj {
                let gvalue = json_to_gvalue(prop_value);
                properties.insert(
                    prop_name.clone(),
                    Property::new(prop_name, gvalue)
                );
            }
        }
    }

    Ok(Edge::new(
        GID::String(id.to_string()),
        label,
        GID::String(in_v.to_string()),
        in_v_label,
        GID::String(out_v.to_string()),
        out_v_label,
        properties,
    ))
}

fn json_to_gvalue(value: &Value) -> GValue {
    match value {
        Value::String(s) => GValue::String(s.clone()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                GValue::Int64(i)
            } else if let Some(f) = n.as_f64() {
                GValue::Double(f)
            } else {
                GValue::Null
            }
        }
        Value::Bool(b) => GValue::Bool(*b),
        Value::Null => GValue::Null,
        Value::Array(arr) => {
            let items: Vec<GValue> = arr.iter().map(json_to_gvalue).collect();
            GValue::List(List::new(items))
        }
        Value::Object(obj) => {
            let mut map = HashMap::new();
            for (k, v) in obj {
                map.insert(k.clone(), json_to_gvalue(v));
            }
            GValue::Map(Map::from(map))
        }
    }
}

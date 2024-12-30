use serde_json::{json, Value};
use std::collections::HashSet;

pub fn is_graph(array: &Vec<Value>) -> Result<(Value, Value), Value> {
    // Prepare vectors for vertices and edges
    let mut vertices = Vec::new();
    let mut edges = Vec::new();
    let mut vertex_ids: HashSet<String> = HashSet::new();
    let mut vertex_ids_needed: HashSet<String> = HashSet::new();

    // Process each element
    for item in array {
        // Each item must be an object
        let obj = match item.as_object() {
            Some(o) => o,
            None => {
                return Err(json!({
                    "error": "Array contains non-object elements",
                    "value": item
                }))
            }
        };

        // Check if it's an edge (has both _from and _to)
        if let (Some(from), Some(to)) = (obj.get("_from"), obj.get("_to")) {
            // Verify _from and _to are strings containing exactly one '/'
            if let (Some(from_str), Some(to_str)) = (from.as_str(), to.as_str()) {
                if from_str.chars().filter(|&c| c == '/').count() != 1
                    || to_str.chars().filter(|&c| c == '/').count() != 1
                {
                    return Err(json!({
                        "error": "Edge _from or _to has invalid format",
                        "value": item
                    }));
                }
                vertex_ids_needed.insert(from_str.to_string());
                vertex_ids_needed.insert(to_str.to_string());
                edges.push(item.clone());
            } else {
                return Err(json!({
                    "error": "Edge _from or _to is not a string",
                    "value": item
                }));
            }
        }
        // Check if it's a vertex (has _id)
        else if let Some(id) = obj.get("_id") {
            // Verify _id is a string containing exactly one '/'
            if let Some(id_str) = id.as_str() {
                if id_str.chars().filter(|&c| c == '/').count() != 1 {
                    return Err(json!({
                        "error": "Vertex _id has invalid format",
                        "value": item
                    }));
                }
                vertex_ids.insert(id_str.to_string());
                vertices.push(item.clone());
            } else {
                return Err(json!({
                    "error": "Vertex _id is not a string",
                    "value": item
                }));
            }
        } else {
            return Err(json!({
                "error": "Object is neither vertex nor edge",
                "value": item
            }));
        }
    }

    // If we got here but found no vertices or edges, it's not a graph
    if edges.is_empty() {
        return Err(json!({
            "error": "Array contains no valid edges",
        }));
    }

    // Now add vertices that occur in edges but are not explicitly mentioned:
    for id in vertex_ids_needed.difference(&vertex_ids) {
        vertices.push(json!({
            "_id": id
        }));
    }

    // Return success with vertices and edges
    Ok((json!(vertices), json!(edges)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_graph() {
        let input = vec![
            json!(
            {
                "_id": "vertices/1",
                "name": "Alice"
            }),
            json!({
                "_id": "vertices/2",
                "name": "Bob"
            }),
            json!({
                "_from": "vertices/1",
                "_to": "vertices/2",
                "type": "knows"
            }),
        ];

        let result = is_graph(&input).unwrap();
        let result_obj = result.as_object().unwrap();

        assert_eq!(
            result_obj
                .get("vertices")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            result_obj.get("edges").unwrap().as_array().unwrap().len(),
            1
        );
    }

    #[test]
    fn test_invalid_input() {
        let input = vec![json!({
            "not": "an array"
        })];

        assert!(is_graph(&input).is_err());
    }

    #[test]
    fn test_invalid_vertex() {
        let input = vec![json!(
            {
                "_id": "invalid_id_no_slash",
                "name": "Alice"
            }
        )];

        assert!(is_graph(&input).is_err());
    }

    #[test]
    fn test_invalid_edge() {
        let input = vec![json!(
            {
                "_from": "invalid/from",
                "_to": "invalid_to_no_slash",
                "type": "knows"
            }
        )];

        assert!(is_graph(&input).is_err());
    }

    #[test]
    fn test_empty_array() {
        let input = vec![];
        assert!(is_graph(&input).is_err());
    }

    #[test]
    fn test_non_graph_objects() {
        let input = vec![json!(
            {
                "just": "a regular object"
            }
        )];

        assert!(is_graph(&input).is_err());
    }
}

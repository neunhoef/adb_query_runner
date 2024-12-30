use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

pub async fn send_to_cytoscape(vertices: &Value, edges: &Value) -> Result<()> {
    let client = Client::new();
    let base_url = "http://localhost:1234/v1";

    // Helper function to collect attributes from a list of objects
    fn collect_attributes(objects: &[Value]) -> HashSet<String> {
        let mut attributes = HashSet::new();
        for obj in objects {
            if let Some(map) = obj.as_object() {
                attributes.extend(
                    map.keys()
                        .filter(|k| !k.starts_with('_')) // Exclude ArangoDB system attributes
                        .cloned(),
                );
            }
        }
        attributes
    }

    // Get vertices and edges as arrays
    let vertices_arr = vertices.as_array().context("Vertices must be an array")?;
    let edges_arr = edges.as_array().context("Edges must be an array")?;

    // Collect vertex and edge attributes
    let vertex_attributes = collect_attributes(vertices_arr);
    let edge_attributes = collect_attributes(edges_arr);

    // Prepare vertices for Cytoscape format
    let cytoscape_vertices: Vec<Value> = vertices_arr
        .iter()
        .filter_map(|v| {
            let obj = v.as_object()?;
            let mut node_data = Map::new();

            // Use _id as node ID
            if let Some(id) = obj.get("_id") {
                node_data.insert("id".to_string(), id.clone());
                node_data.insert("name".to_string(), id.clone()); // Use ID as name by default
            } else {
                return None;
            }

            // Add all other attributes
            for attr in &vertex_attributes {
                if let Some(value) = obj.get(attr) {
                    node_data.insert(attr.clone(), value.clone());
                }
            }

            Some(json!({
                "data": node_data
            }))
        })
        .collect();

    // Prepare edges for Cytoscape format
    let cytoscape_edges: Vec<Value> = edges_arr
        .iter()
        .filter_map(|e| {
            let obj = e.as_object()?;
            let mut edge_data = Map::new();

            // Generate unique edge ID
            if let Some(id) = obj.get("_key") {
                edge_data.insert("id".to_string(), id.clone());
                edge_data.insert("source".to_string(), obj.get("_from").unwrap().clone());
                edge_data.insert("target".to_string(), obj.get("_to").unwrap().clone());
            } else {
                return None;
            }

            // Add all other attributes
            for attr in &edge_attributes {
                if let Some(value) = obj.get(attr) {
                    edge_data.insert(attr.clone(), value.clone());
                }
            }

            Some(json!({
                "data": edge_data
            }))
        })
        .collect();

    // Create network with initial data
    let network_data = json!({
        "format_version": "1.0",
        "generated_by": "adb_query_runner",
        "target_cytoscapejs_version": "~3.0",
        "data": {
            "shared_name": "ArangoDB Graph",
            "name": "ArangoDB Graph"
        },
        "elements": {
            "nodes": cytoscape_vertices,
            "edges": cytoscape_edges
        }
    });

    let network_response: Value = client
        .post(&format!("{}/networks?format=json", base_url))
        .header("Content-Type", "application/json")
        .json(&network_data)
        .send()
        .await?
        .json()
        .await?;

    let network_suid = network_response["networkSUID"]
        .as_i64()
        .context("Failed to get network SUID")?;

    println!("Created network with SUID: {}", network_suid);

    // Create column mappings for vertex attributes
    let mut node_table_columns = HashMap::new();
    for attr in &vertex_attributes {
        // Determine column type based on first non-null value
        let column_type = vertices_arr
            .iter()
            .find_map(|v| {
                v.as_object()?.get(attr).and_then(|val| match val {
                    Value::String(_) => Some("String"),
                    Value::Number(_) => Some("Double"),
                    Value::Bool(_) => Some("Boolean"),
                    _ => None,
                })
            })
            .unwrap_or("String"); // Default to String if type cannot be determined

        node_table_columns.insert(attr, column_type);
    }

    // Create vertex table columns
    for (attr, col_type) in node_table_columns {
        client
            .post(&format!(
                "{}/networks/{}/tables/defaultnode/columns",
                base_url, network_suid
            ))
            .json(&json!({
                "name": attr,
                "type": col_type
            }))
            .send()
            .await?;
    }

    // Create column mappings for edge attributes
    let mut edge_table_columns = HashMap::new();
    for attr in &edge_attributes {
        // Determine column type based on first non-null value
        let column_type = edges_arr
            .iter()
            .find_map(|e| {
                e.as_object()?.get(attr).and_then(|val| match val {
                    Value::String(_) => Some("String"),
                    Value::Number(_) => Some("Double"),
                    Value::Bool(_) => Some("Boolean"),
                    _ => None,
                })
            })
            .unwrap_or("String"); // Default to String if type cannot be determined

        edge_table_columns.insert(attr, column_type);
    }

    // Create edge table columns
    for (attr, col_type) in edge_table_columns {
        client
            .post(&format!(
                "{}/networks/{}/tables/defaultedge/columns",
                base_url, network_suid
            ))
            .json(&json!({
                "name": attr,
                "type": col_type
            }))
            .send()
            .await?;
    }

    // Apply a layout
    client
        .put(&format!(
            "{}/networks/{}/layouts/force-directed",
            base_url, network_suid
        ))
        .send()
        .await?;

    println!("Applied force-directed layout");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cytoscape_integration() {
        let vertices = json!([
            {
                "_id": "vertices/1",
                "name": "Alice",
                "age": 30,
                "active": true
            },
            {
                "_id": "vertices/2",
                "name": "Bob",
                "age": 25,
                "active": false
            }
        ]);

        let edges = json!([
            {
                "_from": "vertices/1",
                "_to": "vertices/2",
                "type": "knows",
                "weight": 0.8,
                "since": "2020"
            }
        ]);

        // Note: This test will only work if Cytoscape is running with CyREST on port 1234
        match send_to_cytoscape(&vertices, &edges).await {
            Ok(_) => println!("Successfully sent graph to Cytoscape"),
            Err(e) => println!("Error sending graph to Cytoscape: {}", e),
        }
    }
}

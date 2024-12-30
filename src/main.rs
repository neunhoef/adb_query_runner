use anyhow::{Context, Result};
use base64::prelude::*;
use include_dir::{include_dir, Dir};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tera::Tera;
use warp::Filter;

mod cytoscape;
mod graph_analyzer;

// Include templates directory at compile time
static TEMPLATES_DIR: Dir = include_dir!("templates");

#[derive(Debug, Serialize, Deserialize, Clone)]
struct QueryParameter {
    name: String,
    parameter_type: String, // Could be "string", "number", "boolean", etc.
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct QueryDefinition {
    name: String,
    description: String,
    query: String,
    parameters: Vec<QueryParameter>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Configuration {
    arangodb_endpoint: String,
    username: String,
    password: String,
    queries: Vec<QueryDefinition>,
}

#[derive(Debug, Serialize)]
struct MenuContext {
    queries: Vec<QueryDefinition>,
}

#[derive(Debug, Serialize)]
struct ParameterFormContext {
    query: QueryDefinition,
    index: usize,
}

#[derive(Debug, Serialize)]
struct ResultContext {
    result_json: String,
    is_it_graph: bool,
}

async fn load_configuration() -> Result<Configuration> {
    let config_str =
        std::fs::read_to_string("config.json").context("Failed to read configuration file")?;
    serde_json::from_str(&config_str).context("Failed to parse configuration")
}

fn setup_tera() -> Result<Tera> {
    let mut tera = Tera::default();

    // Load all templates from the embedded directory
    for file in TEMPLATES_DIR.files() {
        if let Some(name) = file.path().file_name().and_then(|n| n.to_str()) {
            if name.ends_with(".html") {
                let content = std::str::from_utf8(file.contents())?;
                tera.add_raw_template(name, content)?;
            }
        }
    }

    Ok(tera)
}

async fn execute_query(
    config: &Configuration,
    query: &str,
    bind_vars: HashMap<String, serde_json::Value>,
) -> Result<Vec<serde_json::Value>> {
    let client = reqwest::Client::new();

    let auth = BASE64_STANDARD.encode(format!("{}:{}", config.username, config.password));

    let query_request = serde_json::json!({
        "query": query,
        "bindVars": bind_vars,
        "stream": true
    });

    let mut results = Vec::new();
    let response = client
        .post(&format!("{}_api/cursor", config.arangodb_endpoint))
        .header("Authorization", format!("Basic {}", auth))
        .json(&query_request)
        .send()
        .await?;

    let initial_response: serde_json::Value = response.json().await?;
    if let Some(result) = initial_response.get("result").and_then(|r| r.as_array()) {
        results.extend(result.iter().cloned());
    }

    // Handle cursor if more results exist
    if let Some(true) = initial_response.get("hasMore").and_then(|h| h.as_bool()) {
        let cursor_id = initial_response["id"].as_str().unwrap();

        loop {
            let cursor_response = client
                .put(&format!(
                    "{}_api/cursor/{}",
                    config.arangodb_endpoint, cursor_id
                ))
                .header("Authorization", format!("Basic {}", auth))
                .send()
                .await?
                .json::<serde_json::Value>()
                .await?;

            if let Some(result) = cursor_response.get("result").and_then(|r| r.as_array()) {
                results.extend(result.iter().cloned());
            }

            if !cursor_response["hasMore"].as_bool().unwrap_or(false) {
                break;
            }
        }
    }

    Ok(results)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration
    let config = load_configuration().await?;
    let config = Arc::new(config);

    // Setup template engine
    let tera = setup_tera()?;
    let tera = Arc::new(tera);

    // Routes
    let config_filter = warp::any().map(move || Arc::clone(&config));
    let tera_filter = warp::any().map(move || Arc::clone(&tera));

    // Menu page
    let menu = warp::path::end()
        .and(config_filter.clone())
        .and(tera_filter.clone())
        .map(|config: Arc<Configuration>, tera: Arc<Tera>| {
            let context = MenuContext {
                queries: config.queries.clone(),
            };
            let rendered = tera
                .render(
                    "menu.html",
                    &tera::Context::from_serialize(&context).unwrap(),
                )
                .unwrap();
            warp::reply::html(rendered)
        });

    // Parameter form page
    let parameter_form = warp::path!("query" / usize)
        .and(config_filter.clone())
        .and(tera_filter.clone())
        .map(|idx: usize, config: Arc<Configuration>, tera: Arc<Tera>| {
            let query = &config.queries[idx];
            let context = ParameterFormContext {
                query: query.clone(),
                index: idx,
            };
            let rendered = tera.render(
                "parameter_form.html",
                &tera::Context::from_serialize(&context).unwrap(),
            );
            let rendered = rendered.unwrap();
            warp::reply::html(rendered)
        });

    // Execute query and show results
    let execute = warp::path!("execute" / usize)
        .and(warp::post())
        .and(warp::body::form())
        .and(config_filter.clone())
        .and(tera_filter.clone())
        .and_then(
            |idx: usize,
             params: HashMap<String, String>,
             config: Arc<Configuration>,
             tera: Arc<Tera>| async move {
                let query = &config.queries[idx];

                // Convert parameters to proper types based on configuration
                let bind_vars: HashMap<String, serde_json::Value> = params
                    .into_iter()
                    .map(|(k, v)| {
                        let param_type = query
                            .parameters
                            .iter()
                            .find(|p| p.name == k)
                            .map(|p| p.parameter_type.as_str())
                            .unwrap_or("string");

                        let value = match param_type {
                            "number" => serde_json::Value::Number(v.parse().unwrap()),
                            "boolean" => serde_json::Value::Bool(v.parse().unwrap()),
                            _ => serde_json::Value::String(v),
                        };

                        (k, value)
                    })
                    .collect();

                let results = execute_query(&config, &query.query, bind_vars)
                    .await
                    .unwrap();

                let graph_check = graph_analyzer::is_graph(&results);
                let is_it_graph = match graph_check {
                    Ok((v, e)) => {
                        cytoscape::send_to_cytoscape(&v, &e).await.unwrap();

                        true
                    }
                    Err(_e) => false,
                };
                let context = ResultContext {
                    result_json: serde_json::to_string_pretty(&results).unwrap(),
                    is_it_graph,
                };

                let rendered = tera
                    .render(
                        "results.html",
                        &tera::Context::from_serialize(&context).unwrap(),
                    )
                    .unwrap();

                Ok::<_, warp::Rejection>(warp::reply::html(rendered))
            },
        );

    // Serve static files (CSS)
    let css = warp::path("static")
        .and(warp::path("css"))
        .and(warp::path::param())
        .map(|file: String| {
            if let Some(css_file) = TEMPLATES_DIR.get_file(format!("static/css/{}", file)) {
                let content = css_file.contents_utf8().unwrap();
                warp::reply::with_status(
                    warp::reply::with_header(content, "Content-Type", "text/css"),
                    warp::http::StatusCode::OK,
                )
            } else {
                warp::reply::with_status(
                    warp::reply::with_header("CSS file not found", "Content-Type", "text/plain"),
                    warp::http::StatusCode::NOT_FOUND,
                )
            }
        });

    // Combine routes
    let routes = menu.or(parameter_form).or(execute).or(css);

    println!("Server starting on http://localhost:3030");
    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;

    Ok(())
}

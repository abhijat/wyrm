use anyhow::{Context, bail};
use base64::Engine;
use redis::aio::ConnectionManager;
use rmcp::handler::server::router::tool::{ToolRoute, ToolRouter};
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{CallToolResult, Tool};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogEntry {
    name: String,
    description: String,
    command: String,
    parameters: Vec<Parameter>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Parameter {
    name: String,
    #[serde(rename = "type")]
    kind: ParameterType,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ParameterType {
    String,
    StringArray,
}

pub(crate) fn build_routes<S: 'static>(
    connection: ConnectionManager,
) -> anyhow::Result<ToolRouter<S>>
where
    S: rmcp::service::MaybeSend,
{
    let entries: Vec<_> =
        serde_json::from_str(include_str!("../commands.json")).context("invalid commands.json")?;
    let mut router = ToolRouter::new();
    let mut names = HashSet::new();
    for entry in entries {
        validate_entry(&entry)?;
        if !names.insert(entry.name.clone()) {
            bail!("duplicate catalog tool name: {}", entry.name);
        }
        let entry = Arc::new(entry);
        let schema = input_schema(&entry);
        let tool = Tool::new(
            entry.name.clone(),
            entry.description.clone(),
            Arc::new(schema),
        );
        let connection = connection.clone();
        let entry_for_call = entry.clone();
        router.add_route(ToolRoute::new_dyn(
            tool,
            move |context: ToolCallContext<'_, S>| {
                let connection = connection.clone();
                let entry = entry_for_call.clone();
                Box::pin(async move {
                    let arguments = context
                        .arguments
                        .map(Value::Object)
                        .unwrap_or_else(|| json!({}));
                    let args = validate_arguments(&entry, &arguments)
                        .map_err(|error| rmcp::ErrorData::invalid_params(error, None))?;
                    match execute(&connection, &entry, args).await {
                        Ok(response) => Ok(CallToolResult::structured(response).into()),
                        Err(error) => {
                            Ok(CallToolResult::error(vec![rmcp::model::ContentBlock::text(
                                format!("Redis command failed: {error:#}"),
                            )])
                            .into())
                        }
                    }
                })
            },
        ));
    }
    Ok(router)
}

fn validate_entry(entry: &CatalogEntry) -> anyhow::Result<()> {
    if entry.name.is_empty() || entry.command.is_empty() || entry.description.is_empty() {
        bail!("catalog name, description, and command must be nonempty");
    }
    let mut parameter_names = HashSet::new();
    for parameter in &entry.parameters {
        if parameter.name.is_empty() {
            bail!("{}: parameter names must not be empty", entry.name);
        }
        if !parameter_names.insert(&parameter.name) {
            bail!(
                "{}: duplicate parameter name {}",
                entry.name,
                parameter.name
            );
        }
    }
    Ok(())
}

// Parameter order is Redis argument order; string_array expands in place.
fn input_schema(entry: &CatalogEntry) -> rmcp::model::JsonObject {
    let properties = entry
        .parameters
        .iter()
        .map(|parameter| {
            let schema = match parameter.kind {
                ParameterType::String => json!({"type": "string"}),
                ParameterType::StringArray => {
                    json!({"type": "array", "items": {"type": "string"}, "minItems": 1})
                }
            };
            (parameter.name.clone(), schema)
        })
        .collect::<serde_json::Map<_, _>>();
    json!({
        "type": "object",
        "properties": properties,
        "required": entry.parameters.iter().map(|parameter| parameter.name.as_str()).collect::<Vec<_>>(),
        "additionalProperties": false
    })
    .as_object()
    .expect("schema is an object")
    .clone()
}

fn validate_arguments(entry: &CatalogEntry, arguments: &Value) -> Result<Vec<String>, String> {
    let object = arguments
        .as_object()
        .ok_or_else(|| "arguments must be an object".to_owned())?;
    for key in object.keys() {
        if !entry
            .parameters
            .iter()
            .any(|parameter| parameter.name == *key)
        {
            return Err(format!("unknown argument: {key}"));
        }
    }
    let mut args = Vec::new();
    for parameter in &entry.parameters {
        let field = &parameter.name;
        let value = object
            .get(field)
            .ok_or_else(|| format!("missing required argument: {field}"))?;
        match parameter.kind {
            ParameterType::String => args.push(
                value
                    .as_str()
                    .ok_or_else(|| format!("{field} must be a string"))?
                    .to_owned(),
            ),
            ParameterType::StringArray => {
                let values = value
                    .as_array()
                    .ok_or_else(|| format!("{field} must be an array of strings"))?;
                if values.is_empty() {
                    return Err(format!("{field} must contain at least one key"));
                }
                for item in values {
                    args.push(
                        item.as_str()
                            .ok_or_else(|| format!("every {field} item must be a string"))?
                            .to_owned(),
                    );
                }
            }
        }
    }
    Ok(args)
}

async fn execute(
    connection: &ConnectionManager,
    entry: &CatalogEntry,
    args: Vec<String>,
) -> anyhow::Result<Value> {
    let mut command = redis::cmd(&entry.command);
    command.arg(&args);
    let mut connection = connection.clone();
    let value: redis::Value = command.query_async(&mut connection).await?;
    Ok(json!({"value": redis_value(value)?}))
}

fn redis_value(value: redis::Value) -> anyhow::Result<Value> {
    use redis::Value as R;
    Ok(match value {
        R::Nil => Value::Null,
        R::BulkString(bytes) => bytes_value(bytes),
        R::SimpleString(text) => Value::String(text),
        R::Array(items) => Value::Array(
            items
                .into_iter()
                .map(redis_value)
                .collect::<anyhow::Result<_>>()?,
        ),
        other => bail!("unexpected Redis response type: {other:?}"),
    })
}

fn bytes_value(bytes: Vec<u8>) -> Value {
    match String::from_utf8(bytes) {
        Ok(text) => Value::String(text),
        Err(error) => {
            json!({"encoding": "base64", "data": base64::engine::general_purpose::STANDARD.encode(error.into_bytes())})
        }
    }
}

use crate::config::AppConfig;
use rig::prelude::*;
use rig::providers::gemini;
use serde_json::json;

/// Extracts structured data from Gemini / Rig conforming to the given table schema.
pub async fn extract_table_data(
    config: &AppConfig,
    prompt: &str,
    table_name: &str,
    fields: &[String],
    model_override: Option<&str>,
    temperature_override: Option<f64>,
    preamble_override: Option<&str>,
    enable_grounding: bool,
) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    let client = gemini::Client::new(&config.gemini_api_key)?;
    let model = model_override.unwrap_or(&config.model);
    let mut builder = client.agent(model);

    // Build structured field specification and example template
    let field_spec = build_schema_prompt_section(table_name, fields);

    let base_preamble = preamble_override
        .or(config.preamble.as_deref())
        .unwrap_or("You are a helpful structured data extraction assistant.");

    let system_instructions = format!(
        "{base_preamble}\n\n\
        TASK:\n\
        You must perform research and extract factual, high-quality data according to the user request for table '{table_name}'.\n\
        \n\
        SCHEMA REQUIREMENTS:\n\
        {field_spec}\n\
        \n\
        OUTPUT FORMAT REQUIREMENTS:\n\
        1. Return ONLY a valid JSON array of objects `[ {{ ... }}, {{ ... }} ]`.\n\
        2. Do NOT wrap output in markdown code blocks like ```json ... ``` or write any conversational text before or after.\n\
        3. Strictly conform to the schema fields above. Do NOT invent new fields or properties.\n\
        4. Ensure all dates follow 'YYYY-MM-DD' formatting and numbers follow numeric types."
    );

    builder = builder.preamble(&system_instructions);

    if enable_grounding {
        let grounding_config = json!({
            "tools": [
                {
                    "google_search": {}
                }
            ]
        });
        builder = builder.additional_params(grounding_config);
    }

    let temp = temperature_override.or(config.temperature);
    if let Some(t) = temp {
        builder = builder.temperature(t);
    }

    let agent = builder.build();

    tracing::info!(
        model = %model,
        table = %table_name,
        "PopulateTable-Agent: requesting structured data extraction from Gemini agent"
    );

    let raw_response = agent.prompt(prompt).await?;
    let parsed_records = parse_json_response(&raw_response)?;

    tracing::info!(
        count = parsed_records.len(),
        table = %table_name,
        "PopulateTable-Agent: successfully extracted structured records"
    );

    Ok(parsed_records)
}

/// Helper function to parse raw LLM response text into a Vec of JSON objects,
/// handling markdown fences, whitespace, and root object wrappers.
pub fn parse_json_response(raw: &str) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    let mut cleaned = raw.trim();

    // Remove markdown code fences if present
    if cleaned.starts_with("```json") {
        cleaned = cleaned.trim_start_matches("```json");
    } else if cleaned.starts_with("```") {
        cleaned = cleaned.trim_start_matches("```");
    }

    if cleaned.ends_with("```") {
        cleaned = cleaned.trim_end_matches("```");
    }

    cleaned = cleaned.trim();

    // Try parsing as JSON Value
    let val: serde_json::Value = match serde_json::from_str(cleaned) {
        Ok(v) => v,
        Err(e) => {
            // Attempt to locate first '[' and last ']'
            if let (Some(start), Some(end)) = (cleaned.find('['), cleaned.rfind(']')) {
                if start < end {
                    let slice = &cleaned[start..=end];
                    serde_json::from_str(slice)
                        .map_err(|e2| format!("Failed to parse JSON array from slice: {e2} (original error: {e})"))?
                } else {
                    return Err(format!("Failed to parse JSON: {e} | Raw text: {raw}").into());
                }
            } else if let (Some(start), Some(end)) = (cleaned.find('{'), cleaned.rfind('}')) {
                if start < end {
                    let slice = &cleaned[start..=end];
                    serde_json::from_str(slice)
                        .map_err(|e2| format!("Failed to parse JSON object from slice: {e2} (original error: {e})"))?
                } else {
                    return Err(format!("Failed to parse JSON: {e} | Raw text: {raw}").into());
                }
            } else {
                return Err(format!("Failed to parse JSON from LLM: {e} | Raw text: {raw}").into());
            }
        }
    };

    match val {
        serde_json::Value::Array(arr) => Ok(arr),
        serde_json::Value::Object(map) => {
            // Check if any key contains the array of records
            for key in ["records", "data", "items", "results"] {
                if let Some(serde_json::Value::Array(inner_arr)) = map.get(key) {
                    return Ok(inner_arr.clone());
                }
            }
            // Otherwise treat the single object as a 1-element record array
            Ok(vec![serde_json::Value::Object(map)])
        }
        _ => Err("Model response is neither a JSON array nor a JSON object".into()),
    }
}

/// Helper to parse field definitions and construct a clean, human-readable schema specification
/// along with an example JSON template for the LLM.
pub fn build_schema_prompt_section(table_name: &str, fields: &[String]) -> String {
    if fields.is_empty() {
        return "Infer appropriate structured fields from the prompt and user request.".to_string();
    }

    let mut field_descriptions = Vec::new();
    let mut example_map = serde_json::Map::new();

    for f in fields {
        let (name, type_str, default_val) = parse_field_info(f);
        let hint = semantic_hint_for_field(&name, &type_str);

        let mut desc = format!("- `{name}` (type: {type_str})");
        if let Some(def) = default_val {
            desc.push_str(&format!(" [default: {def}]"));
        }
        desc.push_str(&format!(": {hint}"));
        field_descriptions.push(desc);

        if let Some(val) = example_value_for_field(&name, &type_str) {
            example_map.insert(name, val);
        }
    }

    let example_json = serde_json::to_string_pretty(&serde_json::json!([example_map]))
        .unwrap_or_else(|_| "[]".to_string());

    format!(
        "STRICT SCHEMA CONFORMANCE REQUIRED FOR TABLE '{table_name}':\n\
        The target database table is SCHEMAFULL. Every object in the returned JSON array MUST strictly use only the allowed keys listed below:\n\n\
        ALLOWED FIELD DEFINITIONS:\n\
        {}\n\n\
        EXAMPLE JSON STRUCTURE:\n\
        {}\n\n\
        CRITICAL RULES FOR FIELD MAPPING:\n\
        1. ONLY use the exact keys defined above. Do NOT output keys like 'title' or 'incident_title'; place all headlines and incident summaries together into 'raw_text'.\n\
        2. Format all dates as 'YYYY-MM-DD' strings (e.g. \"2000-02-15\").\n\
        3. Store the exact boolean search string used in 'discovery_query'.\n\
        4. Do NOT wrap fields in nested objects or alter column names.",
        field_descriptions.join("\n"),
        example_json
    )
}

pub fn parse_field_info(raw: &str) -> (String, String, Option<String>) {
    let name = if let Some(idx) = raw.find('(') {
        raw[..idx].trim().to_string()
    } else {
        raw.split_whitespace().next().unwrap_or(raw).trim().to_string()
    };

    let mut type_str = "string".to_string();
    let mut default_val = None;

    if let Some(start_paren) = raw.find('(') {
        let ddl = &raw[start_paren + 1..raw.rfind(')').unwrap_or(raw.len())];
        if let Some(type_idx) = ddl.find("TYPE ") {
            let rest = &ddl[type_idx + 5..];
            let after_type = if let Some(def_idx) = rest.find(" DEFAULT ") {
                let (t, rem) = rest.split_at(def_idx);
                let def_str = rem.trim_start_matches(" DEFAULT ");
                default_val = Some(def_str.split(" PERMISSIONS").next().unwrap_or(def_str).trim().to_string());
                t.trim()
            } else if let Some(perm_idx) = rest.find(" PERMISSIONS") {
                rest[..perm_idx].trim()
            } else {
                rest.trim()
            };
            type_str = after_type.to_string();
        }
    }

    (name, type_str, default_val)
}

fn semantic_hint_for_field(name: &str, type_str: &str) -> &'static str {
    match name {
        "url" | "link" => "Canonical URL link to the primary reporting news source or document",
        "discovery_query" | "query" => "The exact boolean search query string used to discover this source document",
        "raw_text" | "content" => "Article headline/title and factual excerpt or quote describing the specific crime incident",
        "incident_date" | "crime_date" | "date" => "Date when the incident/crime occurred in ISO-8601 YYYY-MM-DD format",
        "status" => "Initial processing lifecycle status; defaults to 'Pending'",
        "crime_id" => "Optional record pointer to parent crime record (omit or null if unlinked)",
        "fetched_at" => "Timestamp when retrieved (defaults to current time if omitted)",
        _ => {
            if type_str.contains("datetime") {
                "ISO-8601 date string (YYYY-MM-DD)"
            } else if type_str.contains("int") || type_str.contains("number") {
                "Numeric value"
            } else {
                "Factual data string"
            }
        }
    }
}

fn example_value_for_field(name: &str, type_str: &str) -> Option<serde_json::Value> {
    match name {
        "url" => Some(serde_json::json!("https://www.example-news.de/incident-report.html")),
        "discovery_query" => Some(serde_json::json!("\"Berlin\" homicide site:spiegel.de")),
        "raw_text" => Some(serde_json::json!("Headline: Factual summary and quotation describing the criminal incident...")),
        "incident_date" => Some(serde_json::json!("2000-02-15")),
        "status" => Some(serde_json::json!("Pending")),
        "crime_id" => None,
        "fetched_at" => None,
        _ => {
            if type_str.contains("datetime") {
                Some(serde_json::json!("2000-02-15"))
            } else if type_str.contains("int") {
                Some(serde_json::json!(100))
            } else if type_str.contains("float") || type_str.contains("number") {
                Some(serde_json::json!(12.5))
            } else if type_str.contains("bool") {
                Some(serde_json::json!(true))
            } else {
                Some(serde_json::json!("example value"))
            }
        }
    }
}

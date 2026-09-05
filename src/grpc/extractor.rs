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

    // Build field specification
    let field_spec = if fields.is_empty() {
        "Infer appropriate structured fields from the prompt and user request.".to_string()
    } else {
        format!(
            "Each object in the array MUST adhere to the following field definitions:\n{}",
            fields
                .iter()
                .map(|f| format!("- {f}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };

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
        3. Ensure all dates and numbers follow valid formatting (e.g. ISO-8601 for dates)."
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
        prompt = %prompt,
        "Requesting structured data extraction from Gemini agent"
    );

    let raw_response = agent.prompt(prompt).await?;
    let parsed_records = parse_json_response(&raw_response)?;

    tracing::info!(
        count = parsed_records.len(),
        table = %table_name,
        "Successfully extracted structured records"
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

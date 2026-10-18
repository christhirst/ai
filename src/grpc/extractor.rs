use crate::config::{AppConfig, ModelProvider};
use crate::grpc::file_attachment::DocumentAttachment;
use rig::completion::message::{Document, DocumentMediaType, DocumentSourceKind, UserContent};
use rig::completion::Message;
use rig::prelude::*;
use rig::providers::{gemini, openai};
use serde_json::json;

async fn dispatch_agent_prompt<M>(
    agent: &rig::agent::Agent<M>,
    prompt: &str,
    attachments: &[DocumentAttachment],
) -> Result<String, Box<dyn std::error::Error>>
where
    M: rig::completion::CompletionModel + 'static,
{
    if attachments.is_empty() {
        let response = agent.prompt(prompt.to_string()).await?;
        return Ok(response);
    }

    // 1. Combine text attachments into prompt
    let mut combined_prompt = prompt.to_string();
    for att in attachments {
        if let DocumentAttachment::Text { content, filename } = att {
            let fname = filename.as_deref().unwrap_or("document");
            combined_prompt.push_str(&format!(
                "\n\n--- Document: {fname} ---\n{content}\n--- End Document ---\n"
            ));
        }
    }

    // 2. Collect any multimodal PDF documents
    let pdf_docs: Vec<Document> = attachments
        .iter()
        .filter_map(|att| match att {
            DocumentAttachment::Pdf { base64_data, .. } => Some(Document {
                data: DocumentSourceKind::Base64(base64_data.clone()),
                media_type: Some(DocumentMediaType::PDF),
                additional_params: None,
            }),
            _ => None,
        })
        .collect();

    if pdf_docs.is_empty() {
        let response = agent.prompt(combined_prompt).await?;
        Ok(response)
    } else {
        let mut user_parts = vec![UserContent::text(combined_prompt)];
        for doc in pdf_docs {
            user_parts.push(UserContent::Document(doc));
        }
        let user_content = rig::OneOrMany::many(user_parts)
            .map_err(|e| format!("Failed to build user content: {e}"))?;

        let message = Message::User {
            content: user_content,
        };
        let response = agent.prompt(message).await?;
        Ok(response)
    }
}

/// Extracts structured data from Gemini / Qwen / Local1 / Local2 conforming to the given table schema.
#[allow(clippy::too_many_arguments)]
pub async fn extract_table_data(
    config: &AppConfig,
    prompt: &str,
    table_name: &str,
    fields: &[String],
    provider_override: Option<&str>,
    model_override: Option<&str>,
    temperature_override: Option<f64>,
    preamble_override: Option<&str>,
    enable_grounding: bool,
    thinking_level_override: Option<&str>,
    base_url_override: Option<&str>,
    attachments: &[DocumentAttachment],
) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    let provider = match provider_override {
        Some(p) if !p.trim().is_empty() => p
            .parse::<ModelProvider>()
            .map_err(Box::<dyn std::error::Error>::from)?,
        _ => config.provider,
    };

    let model = match model_override.filter(|m| !m.trim().is_empty()) {
        Some(m) => m.to_string(),
        None => match provider {
            ModelProvider::Gemini => config.model.clone(),
            ModelProvider::Qwen => {
                if config.model.contains("gemini") {
                    config.qwen_model.clone()
                } else {
                    config.model.clone()
                }
            }
            ModelProvider::Local1 => config
                .local1
                .as_ref()
                .and_then(|c| c.model.clone())
                .unwrap_or_else(|| "default".to_string()),
            ModelProvider::Local2 => config
                .local2
                .as_ref()
                .and_then(|c| c.model.clone())
                .unwrap_or_else(|| "default".to_string()),
        },
    };

    // If text attachments are provided, incorporate their decoded UTF-8 contents into the prompt
    let effective_prompt = if !attachments.is_empty() {
        let mut text_parts = Vec::new();
        for att in attachments {
            if let DocumentAttachment::Text { content, filename } = att {
                let file_label = filename
                    .as_deref()
                    .map(|n| format!(" (from file '{n}')"))
                    .unwrap_or_default();
                text_parts.push(format!(
                    "DOCUMENT CONTENT{file_label}:\n\
                    ====================\n\
                    {content}\n\
                    ===================="
                ));
            }
        }
        if text_parts.is_empty() {
            prompt.to_string()
        } else {
            format!(
                "{}\n\nEXTRACTION INSTRUCTION:\n{prompt}",
                text_parts.join("\n\n")
            )
        }
    } else {
        prompt.to_string()
    };

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
        4. Ensure all dates follow 'YYYY-MM-DD' formatting and numbers follow numeric types.\n\
        5. If no records or incidents are found matching the criteria for the specified timeframe, return an empty JSON array: `[]`. Do NOT return empty text or any conversational explanation."
    );

    let raw_response = match provider {
        ModelProvider::Gemini => {
            // Enforce Gemini 3+ models only; older legacy models are prevented
            if !model.contains("gemini-3") {
                return Err(format!(
                    "Unsupported model '{model}': only Gemini 3+ models (e.g. gemini-3.7-flash, gemini-3.8-flash) are supported for Gemini provider."
                ).into());
            }

            if config.gemini_api_key.trim().is_empty() {
                return Err("Gemini API key is not configured. Please set GEMINI_API_KEY, gemini_api_key in config, or OpenBao/Vault".into());
            }

            let client = gemini::Client::new(&config.gemini_api_key)?;
            let mut builder = client.agent(&model);
            builder = builder.preamble(&system_instructions);

            // Validate thinking level if specified
            let normalized_thinking_level = match thinking_level_override
                .map(|s| s.trim().to_lowercase())
            {
                Some(s) if s.is_empty() => None,
                Some(s) => match s.as_str() {
                    "minimal" | "low" | "medium" | "high" => Some(s),
                    other => {
                        return Err(format!(
                            "Invalid thinking_level '{other}': allowed values are 'minimal', 'low', 'medium', 'high'."
                        ).into());
                    }
                },
                None => None,
            };

            // Default to "low" if grounding is enabled and no thinking level is explicitly provided,
            // to prevent runaway 10,000+ token thought loops that exhaust the turn.
            let effective_thinking_level = match (normalized_thinking_level, enable_grounding) {
                (Some(level), _) => Some(level),
                (None, true) => Some("low".to_string()),
                (None, false) => None,
            };

            let mut additional_params = serde_json::Map::new();

            if enable_grounding {
                additional_params.insert("tools".to_string(), json!([{"google_search": {}}]));
            }

            let mut gen_config = serde_json::Map::new();
            if let Some(level) = effective_thinking_level {
                gen_config.insert(
                    "thinkingConfig".to_string(),
                    json!({"thinkingLevel": level}),
                );
            }

            if !gen_config.is_empty() {
                additional_params.insert(
                    "generationConfig".to_string(),
                    serde_json::Value::Object(gen_config),
                );
            }

            if !additional_params.is_empty() {
                builder = builder.additional_params(serde_json::Value::Object(additional_params));
            }

            let temp = temperature_override.or(config.temperature);
            if let Some(t) = temp {
                builder = builder.temperature(t);
            }

            let agent = builder.build();

            tracing::info!(
                provider = "gemini",
                model = %model,
                table = %table_name,
                has_attachment = !attachments.is_empty(),
                "PopulateTable-Agent: requesting structured data extraction from Gemini agent"
            );

            dispatch_agent_prompt(&agent, &effective_prompt, attachments).await?
        }
        ModelProvider::Qwen => {
            if config.qwen_api_key.trim().is_empty() {
                return Err("Qwen API key is not configured. Please set QWEN_API_KEY / DASHSCOPE_API_KEY, qwen_api_key in config, or OpenBao/Vault".into());
            }

            let effective_base_url = base_url_override
                .map(|u| u.trim().trim_end_matches('/'))
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| config.qwen_base_url.trim_end_matches('/'));

            let client = openai::CompletionsClient::builder()
                .api_key(&config.qwen_api_key)
                .base_url(effective_base_url)
                .build()?;

            let mut builder = client.agent(&model);
            builder = builder.preamble(&system_instructions);

            // Web search is passed ONLY when enable_grounding is true
            if enable_grounding {
                builder = builder.additional_params(json!({
                    "enable_search": true,
                    "search_options": {
                        "forced_search": true,
                        "search_strategy": "max",
                        "enable_source": true
                    }
                }));
            }

            let temp = temperature_override.or(config.temperature);
            if let Some(t) = temp {
                builder = builder.temperature(t);
            }

            let agent = builder.build();

            tracing::info!(
                provider = "qwen",
                model = %model,
                base_url = %effective_base_url,
                table = %table_name,
                grounding = enable_grounding,
                has_attachment = !attachments.is_empty(),
                "PopulateTable-Agent: requesting structured data extraction from Qwen agent"
            );

            dispatch_agent_prompt(&agent, &effective_prompt, attachments).await?
        }
        ModelProvider::Local1 | ModelProvider::Local2 => {
            let local_cfg = match provider {
                ModelProvider::Local1 => config.local1.as_ref(),
                ModelProvider::Local2 => config.local2.as_ref(),
                _ => None,
            };

            let effective_base_url = base_url_override
                .map(|u| u.trim().trim_end_matches('/'))
                .filter(|u| !u.is_empty())
                .map(ToString::to_string)
                .or_else(|| {
                    local_cfg.and_then(|c| {
                        c.base_url
                            .as_deref()
                            .map(|u| u.trim_end_matches('/').to_string())
                    })
                })
                .ok_or_else(|| {
                    format!(
                        "Base URL is not configured for provider '{provider}'. Please specify base_url in config [{provider}] or in request."
                    )
                })?;

            let api_key = local_cfg
                .and_then(|c| c.api_key.as_deref())
                .filter(|k| !k.trim().is_empty())
                .unwrap_or("none");

            let client = openai::CompletionsClient::builder()
                .api_key(api_key)
                .base_url(&effective_base_url)
                .build()?;

            let mut builder = client.agent(&model);
            builder = builder.preamble(&system_instructions);

            let temp = temperature_override.or(config.temperature);
            if let Some(t) = temp {
                builder = builder.temperature(t);
            }

            let agent = builder.build();

            tracing::info!(
                provider = %provider,
                model = %model,
                base_url = %effective_base_url,
                table = %table_name,
                has_attachment = !attachments.is_empty(),
                "PopulateTable-Agent: requesting structured data extraction from local agent"
            );

            dispatch_agent_prompt(&agent, &effective_prompt, attachments).await?
        }
    };

    let parsed_records = parse_json_response(&raw_response)?;

    tracing::info!(
        provider = %provider,
        count = parsed_records.len(),
        table = %table_name,
        "PopulateTable-Agent: successfully extracted structured records"
    );

    Ok(parsed_records)
}


/// Helper function to parse raw LLM response text into a Vec of JSON objects,
/// handling markdown fences, whitespace, and root object wrappers.
pub fn parse_json_response(
    raw: &str,
) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
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
                    serde_json::from_str(slice).map_err(|e2| {
                        format!("Failed to parse JSON array from slice: {e2} (original error: {e})")
                    })?
                } else {
                    return Err(format!("Failed to parse JSON: {e} | Raw text: {raw}").into());
                }
            } else if let (Some(start), Some(end)) = (cleaned.find('{'), cleaned.rfind('}')) {
                if start < end {
                    let slice = &cleaned[start..=end];
                    serde_json::from_str(slice).map_err(|e2| {
                        format!(
                            "Failed to parse JSON object from slice: {e2} (original error: {e})"
                        )
                    })?
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
        raw.split_whitespace()
            .next()
            .unwrap_or(raw)
            .trim()
            .to_string()
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
                default_val = Some(
                    def_str
                        .split(" PERMISSIONS")
                        .next()
                        .unwrap_or(def_str)
                        .trim()
                        .to_string(),
                );
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
        "discovery_query" | "query" => {
            "The exact boolean search query string used to discover this source document"
        }
        "raw_text" | "content" => {
            "Article headline/title and factual excerpt or quote describing the specific crime incident"
        }
        "incident_date" | "crime_date" | "date" => {
            "Date when the incident/crime occurred in ISO-8601 YYYY-MM-DD format"
        }
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
        "url" => Some(serde_json::json!(
            "https://www.example-news.de/incident-report.html"
        )),
        "discovery_query" => Some(serde_json::json!("\"Berlin\" homicide site:spiegel.de")),
        "raw_text" => Some(serde_json::json!(
            "Headline: Factual summary and quotation describing the criminal incident..."
        )),
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

/// Fact-checking outcome returned from an LLM.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FactCheckOutcome {
    pub verdict: String,
    pub status: String,
    pub explanation: String,
    pub sources: Vec<String>,
}

/// Fact-checks a single database record by querying Gemini or Qwen with web grounding enabled.
#[allow(clippy::too_many_arguments)]
pub async fn fact_check_record(
    config: &AppConfig,
    record_json: &str,
    table_name: &str,
    provider_override: Option<&str>,
    model_override: Option<&str>,
    temperature_override: Option<f64>,
    preamble_override: Option<&str>,
    thinking_level_override: Option<&str>,
    base_url_override: Option<&str>,
) -> Result<FactCheckOutcome, Box<dyn std::error::Error>> {
    let provider = match provider_override {
        Some(p) if !p.trim().is_empty() => p
            .parse::<ModelProvider>()
            .map_err(Box::<dyn std::error::Error>::from)?,
        _ => config.provider,
    };

    let model = match model_override.filter(|m| !m.trim().is_empty()) {
        Some(m) => m.to_string(),
        None => match provider {
            ModelProvider::Gemini => config.model.clone(),
            ModelProvider::Qwen => {
                if config.model.contains("gemini") {
                    config.qwen_model.clone()
                } else {
                    config.model.clone()
                }
            }
            ModelProvider::Local1 => config
                .local1
                .as_ref()
                .and_then(|c| c.model.clone())
                .unwrap_or_else(|| "default".to_string()),
            ModelProvider::Local2 => config
                .local2
                .as_ref()
                .and_then(|c| c.model.clone())
                .unwrap_or_else(|| "default".to_string()),
        },
    };

    let base_preamble = preamble_override
        .or(config.preamble.as_deref())
        .unwrap_or("You are a meticulous investigative fact-checking assistant.");

    let system_instructions = format!(
        "{base_preamble}\n\n\
        TASK:\n\
        You must fact-check the factual claims in the provided record for table '{table_name}'.\n\
        Search the web for authoritative reporting, official police/court releases, or credible sources.\n\
        \n\
        CRITICAL DISTINCTIONS:\n\
        - 'incident_date': When the reported event, crime, or incident actually happened.\n\
        - 'fetched_at': When this record was originally retrieved or extracted by the system.\n\
        \n\
        OUTPUT FORMAT:\n\
        Return ONLY a single valid JSON object (no markdown code blocks, no other text):\n\
        {{\n\
          \"verdict\": \"confirmed\" | \"disputed\" | \"unverifiable\",\n\
          \"status\": \"Checked\" | \"Discarded_Irrelevant\",\n\
          \"explanation\": \"Detailed explanation citing facts found during research\",\n\
          \"sources\": [\"https://...\"]\n\
        }}\n\
        \n\
        VERDICT & STATUS RULES:\n\
        1. If the core factual claims are corroborated by credible sources: verdict=\"confirmed\", status=\"Checked\".\n\
        2. If the claims are contradicted, false, fabricated, or a known hoax: verdict=\"disputed\", status=\"Discarded_Irrelevant\".\n\
        3. If there is insufficient credible evidence online to verify or the record is irrelevant: verdict=\"unverifiable\", status=\"Discarded_Irrelevant\"."
    );

    let prompt = format!(
        "Please fact-check this database record from table '{table_name}':\n\n\
        ```json\n\
        {record_json}\n\
        ```\n\n\
        Investigate whether the incident, facts, locations, and dates reported here are accurate and corroborated by credible online sources.\n\
        Return ONLY the requested JSON object."
    );

    let raw_response = match provider {
        ModelProvider::Gemini => {
            if !model.contains("gemini-3") {
                return Err(format!(
                    "Unsupported model '{model}': only Gemini 3+ models (e.g. gemini-3.7-flash, gemini-3.8-flash) are supported for Gemini provider."
                ).into());
            }

            if config.gemini_api_key.trim().is_empty() {
                return Err("Gemini API key is not configured. Please set GEMINI_API_KEY, gemini_api_key in config, or OpenBao/Vault".into());
            }

            let client = gemini::Client::new(&config.gemini_api_key)?;
            let mut builder = client.agent(&model);
            builder = builder.preamble(&system_instructions);

            let normalized_thinking_level = match thinking_level_override
                .map(|s| s.trim().to_lowercase())
            {
                Some(s) if s.is_empty() => None,
                Some(s) => match s.as_str() {
                    "minimal" | "low" | "medium" | "high" => Some(s),
                    other => {
                        return Err(format!(
                            "Invalid thinking_level '{other}': allowed values are 'minimal', 'low', 'medium', 'high'."
                        ).into());
                    }
                },
                None => None,
            };

            let effective_thinking_level =
                normalized_thinking_level.unwrap_or_else(|| "low".to_string());

            let mut additional_params = serde_json::Map::new();
            additional_params.insert("tools".to_string(), json!([{"google_search": {}}]));

            let mut gen_config = serde_json::Map::new();
            gen_config.insert(
                "thinkingConfig".to_string(),
                json!({"thinkingLevel": effective_thinking_level}),
            );
            additional_params.insert(
                "generationConfig".to_string(),
                serde_json::Value::Object(gen_config),
            );

            builder = builder.additional_params(serde_json::Value::Object(additional_params));

            let temp = temperature_override.or(config.temperature);
            if let Some(t) = temp {
                builder = builder.temperature(t);
            }

            let agent = builder.build();

            tracing::info!(
                provider = "gemini",
                model = %model,
                table = %table_name,
                "FactCheck-Agent: requesting factual verification from Gemini agent"
            );

            agent.prompt(&prompt).await?
        }
        ModelProvider::Qwen => {
            if config.qwen_api_key.trim().is_empty() {
                return Err("Qwen API key is not configured. Please set QWEN_API_KEY / DASHSCOPE_API_KEY, qwen_api_key in config, or OpenBao/Vault".into());
            }

            let effective_base_url = base_url_override
                .map(|u| u.trim().trim_end_matches('/'))
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| config.qwen_base_url.trim_end_matches('/'));

            let client = openai::CompletionsClient::builder()
                .api_key(&config.qwen_api_key)
                .base_url(effective_base_url)
                .build()?;

            let mut builder = client.agent(&model);
            builder = builder.preamble(&system_instructions);

            builder = builder.additional_params(json!({
                "enable_search": true,
                "search_options": {
                    "forced_search": true,
                    "search_strategy": "max",
                    "enable_source": true
                }
            }));

            let temp = temperature_override.or(config.temperature);
            if let Some(t) = temp {
                builder = builder.temperature(t);
            }

            let agent = builder.build();

            tracing::info!(
                provider = "qwen",
                model = %model,
                base_url = %effective_base_url,
                table = %table_name,
                "FactCheck-Agent: requesting factual verification from Qwen agent"
            );

            agent.prompt(&prompt).await?
        }
        ModelProvider::Local1 | ModelProvider::Local2 => {
            let local_cfg = match provider {
                ModelProvider::Local1 => config.local1.as_ref(),
                ModelProvider::Local2 => config.local2.as_ref(),
                _ => None,
            };

            let effective_base_url = base_url_override
                .map(|u| u.trim().trim_end_matches('/'))
                .filter(|u| !u.is_empty())
                .map(ToString::to_string)
                .or_else(|| {
                    local_cfg.and_then(|c| {
                        c.base_url
                            .as_deref()
                            .map(|u| u.trim_end_matches('/').to_string())
                    })
                })
                .ok_or_else(|| {
                    format!(
                        "Base URL is not configured for provider '{provider}'. Please specify base_url in config [{provider}] or in request."
                    )
                })?;

            let api_key = local_cfg
                .and_then(|c| c.api_key.as_deref())
                .filter(|k| !k.trim().is_empty())
                .unwrap_or("none");

            let client = openai::CompletionsClient::builder()
                .api_key(api_key)
                .base_url(&effective_base_url)
                .build()?;

            let mut builder = client.agent(&model);
            builder = builder.preamble(&system_instructions);

            let temp = temperature_override.or(config.temperature);
            if let Some(t) = temp {
                builder = builder.temperature(t);
            }

            let agent = builder.build();

            tracing::info!(
                provider = %provider,
                model = %model,
                base_url = %effective_base_url,
                table = %table_name,
                "FactCheck-Agent: requesting factual verification from local agent"
            );

            agent.prompt(&prompt).await?
        }
    };

    let parsed_records = parse_json_response(&raw_response)?;
    let first = parsed_records
        .into_iter()
        .next()
        .ok_or("Fact-check response did not contain a valid JSON object")?;

    let verdict = first
        .get("verdict")
        .and_then(|v| v.as_str())
        .unwrap_or("unverifiable")
        .trim()
        .to_lowercase();

    let status = if verdict == "confirmed" {
        "Checked".to_string()
    } else {
        "Discarded_Irrelevant".to_string()
    };

    let explanation = first
        .get("explanation")
        .and_then(|v| v.as_str())
        .unwrap_or("No explanation provided")
        .to_string();

    let sources = first
        .get("sources")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|s| s.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    Ok(FactCheckOutcome {
        verdict,
        status,
        explanation,
        sources,
    })
}

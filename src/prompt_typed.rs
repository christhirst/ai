#![allow(non_snake_case)]

use crate::config::AppConfig;
use rig::prelude::*;
use rig::providers::gemini;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use surrealdb_types::SurrealValue;

/// Strongly-typed structured record for GDP data analysis.
#[derive(Deserialize, Serialize, JsonSchema, SurrealValue, Debug, Clone, PartialEq)]
pub struct GdpRecord {
    /// The GDP value (in trillions USD)
    pub gdp: f64,
    /// The four-digit calendar year (e.g., "1990")
    pub year: String,
}

#[derive(Deserialize, Serialize, JsonSchema, SurrealValue, Debug, Clone, PartialEq)]
#[allow(non_snake_case)]
pub struct Homecides {
    /// The city where the homicide occurred (e.g. "Berlin")
    pub citiy: String,
    /// The citizenship or nationality of the suspect/perpetrator (e.g. "German")
    pub Citizenship: String,
    /// The date and time of the incident in ISO-8601 format (e.g. "2026-01-15T12:00:00Z")
    #[schemars(with = "String")]
    pub Date: surrealdb_types::Datetime,
    /// The weapon or method used (e.g. "Knife", "Firearm")
    pub weapon: String,
    /// The prison sentence duration (e.g. "10y", "15y", "0s")
    #[schemars(with = "String")]
    #[serde(deserialize_with = "deserialize_prison_duration")]
    pub Prison_time: surrealdb_types::Duration,
    /// Any other sentence, fine, or probation notes
    pub Other_sentence: String,
}

fn deserialize_prison_duration<'de, D>(deserializer: D) -> Result<surrealdb_types::Duration, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct PrisonDurationVisitor;

    impl<'de> serde::de::Visitor<'de> for PrisonDurationVisitor {
        type Value = surrealdb_types::Duration;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a duration string (e.g. '15y', '0s') or duration struct")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            let trimmed = v.trim();
            if trimmed.is_empty() || trimmed == "0" || trimmed.eq_ignore_ascii_case("none") {
                return Ok(surrealdb_types::Duration::ZERO);
            }
            trimmed.parse::<surrealdb_types::Duration>().or_else(|_| {
                let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(num) = digits.parse::<u64>() {
                    if trimmed.contains('y') || trimmed.contains("year") {
                        return Ok(surrealdb_types::Duration::from_secs(num * 365 * 24 * 3600));
                    }
                    if trimmed.contains('m') || trimmed.contains("month") {
                        return Ok(surrealdb_types::Duration::from_secs(num * 30 * 24 * 3600));
                    }
                    if trimmed.contains('d') || trimmed.contains("day") {
                        return Ok(surrealdb_types::Duration::from_secs(num * 24 * 3600));
                    }
                    return Ok(surrealdb_types::Duration::from_secs(num));
                }
                Ok(surrealdb_types::Duration::ZERO)
            })
        }

        fn visit_newtype_struct<D2>(self, deserializer: D2) -> Result<Self::Value, D2::Error>
        where
            D2: serde::Deserializer<'de>,
        {
            let std_dur = std::time::Duration::deserialize(deserializer)?;
            Ok(surrealdb_types::Duration::from_std(std_dur))
        }

        fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
        where
            M: serde::de::MapAccess<'de>,
        {
            let std_dur = std::time::Duration::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
            Ok(surrealdb_types::Duration::from_std(std_dur))
        }
    }

    deserializer.deserialize_any(PrisonDurationVisitor)
}

/// Executes the typed structured output prompt variant and persists records into SurrealDB.
pub async fn run(config: &AppConfig) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
    // Define the native Google Search grounding config
    let grounding_config = json!({
        "tools": [
            {
                "google_search": {}
            }
        ]
    });

    // 1. Connect to SurrealDB and introspect existing schema
    println!(
        "Connecting to SurrealDB (endpoint: {}, ns: {}, db: {})...",
        config.db.endpoint, config.db.namespace, config.db.database
    );
    let db = crate::db::init_db_from_config(&config.db).await?;
    println!("Connected to SurrealDB successfully.");

    let schema_info = crate::db::get_db_info(&db).await?;
    println!("Database Schema Info (INFO FOR DB):");
    println!("{}\n", serde_json::to_string_pretty(&schema_info)?);

    // 2. Request structured data from Gemini with Google Search Grounding
    let client = gemini::Client::new(&config.gemini_api_key)?;

    let mut builder = client.agent(&config.model);

    let system_instructions = match &config.preamble {
        Some(p) => format!("{p}\n\nSearch guidelines: Search online for all reported homicides, fatal stabbings, shootings, and manslaughter cases in Berlin during January 2026. For each case, return citiy ('Berlin'), Citizenship of suspect/perpetrator if reported (otherwise 'Unknown'), Date (ISO-8601 format e.g. '2026-01-15T00:00:00Z'), weapon used, Prison_time (use '0s' if trial/sentencing is pending), and Other_sentence (e.g. 'Under investigation', 'Arrested', 'Suspect at large', or trial details)."),
        None => "You are an investigative researcher. Search online for all reported homicides, fatal stabbings, shootings, and manslaughter cases in Berlin during January 2026. For each case, return citiy ('Berlin'), Citizenship of suspect/perpetrator if reported (otherwise 'Unknown'), Date (ISO-8601 format e.g. '2026-01-15T00:00:00Z'), weapon used, Prison_time (use '0s' if trial/sentencing is pending), and Other_sentence (e.g. 'Under investigation', 'Arrested', 'Suspect at large', or trial details).".to_string(),
    };

    builder = builder
        .preamble(&system_instructions)
        .additional_params(grounding_config);

    if let Some(temperature) = config.temperature {
        builder = builder.temperature(temperature);
    }

    let agent = builder.build();

    println!("Model: {}", config.model);
    println!("Requesting structured data from Gemini...");

    let response: Vec<Homecides> = agent.prompt_typed(&config.prompt_typed.query).await?;

    println!(
        "\nTyped Agent Response ({} records received):",
        response.len()
    );
    for item in &response {
        println!(
            "  • City: {} | Date: {} | Weapon: {} | Citizenship: {} | Prison: {} | Other: {}",
            item.citiy,
            item.Date,
            item.weapon,
            item.Citizenship,
            item.Prison_time,
            item.Other_sentence
        );
    }

    // 3. Persist received records into SurrealDB
    let saved = crate::db::create_homecide_records(&db, &response).await?;
    println!(
        "\nSuccessfully stored {} records into SurrealDB [{}/{}] (table: '{}')",
        saved.len(),
        config.db.namespace,
        config.db.database,
        crate::db::TABLE_HOMECIDES
    );

    // 4. Verify persisted records from SurrealDB
    let all_db_records = crate::db::get_all_homecides(&db).await?;
    println!(
        "Current total records in table '{}': {}",
        crate::db::TABLE_HOMECIDES,
        all_db_records.len()
    );

    Ok(response)
}

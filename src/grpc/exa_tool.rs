use rig::tool::{PortableTool, ToolErrorKind, ToolExecutionError};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Error type for Exa.ai tool invocations.
#[derive(Debug, thiserror::Error)]
pub enum ExaToolError {
    #[error("Invalid Exa tool arguments: {0}")]
    InvalidArgs(String),

    #[error("Exa HTTP request error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Exa API returned HTTP {status}: {body}")]
    Api { status: u16, body: String },
}

/// Arguments for the `exa_search` tool.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExaSearchArgs {
    /// The search query string (natural language or keyword query).
    pub query: String,
    /// Number of search results to return (default: 10, max: 25).
    #[serde(default)]
    pub num_results: Option<u32>,
    /// Optional start published date filter in YYYY-MM-DD or ISO-8601 format (e.g. "2000-01-01").
    #[serde(default)]
    pub start_published_date: Option<String>,
    /// Optional end published date filter in YYYY-MM-DD or ISO-8601 format (e.g. "2000-12-31").
    #[serde(default)]
    pub end_published_date: Option<String>,
    /// Optional list of domains to include (e.g. ["spiegel.de", "zeit.de"]).
    #[serde(default)]
    pub include_domains: Option<Vec<String>>,
    /// Optional list of domains to exclude.
    #[serde(default)]
    pub exclude_domains: Option<Vec<String>>,
    /// Search type: "auto", "neural", or "keyword" (default: "auto").
    #[serde(default)]
    pub search_type: Option<String>,
    /// Optional category filter (e.g. "news", "pdf", "research paper", "company").
    #[serde(default)]
    pub category: Option<String>,
}

/// A single search or content result returned by Exa.ai.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ExaResultItem {
    #[serde(default)]
    pub title: Option<String>,
    pub url: String,
    #[serde(default, alias = "publishedDate")]
    pub published_date: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub highlights: Option<Vec<String>>,
}

/// Output returned by `ExaSearchTool`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExaSearchOutput {
    pub query: String,
    pub results: Vec<ExaResultItem>,
}

/// Arguments for the `exa_get_contents` tool.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExaContentsArgs {
    /// List of URLs to retrieve full page contents for.
    pub urls: Vec<String>,
    /// Maximum characters of text to extract per URL (default: 5000).
    #[serde(default)]
    pub max_characters: Option<u32>,
}

/// Output returned by `ExaContentsTool`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExaContentsOutput {
    pub results: Vec<ExaResultItem>,
}

#[derive(Debug, Deserialize)]
struct RawExaResponse {
    #[serde(default)]
    results: Vec<ExaResultItem>,
}

/// Normalizes a date string for Exa's `startPublishedDate` / `endPublishedDate` fields.
/// If the input is a bare `YYYY-MM-DD` date, appends `T00:00:00.000Z` (start) or `T23:59:59.999Z` (end).
pub fn normalize_exa_date(date_str: &str, is_end: bool) -> Option<String> {
    let trimmed = date_str.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() == 10
        && trimmed.as_bytes().get(4) == Some(&b'-')
        && trimmed.as_bytes().get(7) == Some(&b'-')
    {
        if is_end {
            Some(format!("{trimmed}T23:59:59.999Z"))
        } else {
            Some(format!("{trimmed}T00:00:00.000Z"))
        }
    } else {
        Some(trimmed.to_string())
    }
}

/// Rig tool that searches the web using Exa.ai (`/search`) and returns snippets, text, and highlights.
#[derive(Clone)]
pub struct ExaSearchTool {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl ExaSearchTool {
    pub fn new(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        let base_url = base_url.into().trim().trim_end_matches('/').to_string();
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into().trim().to_string(),
            base_url: if base_url.is_empty() {
                "https://api.exa.ai".to_string()
            } else {
                base_url
            },
        }
    }
}

impl PortableTool for ExaSearchTool {
    const NAME: &'static str = "exa_search";
    type Args = ExaSearchArgs;
    type Output = ExaSearchOutput;
    type Error = ExaToolError;

    fn description(&self) -> String {
        "Search the web using Exa.ai for primary news reports, articles, official releases, and documents. \
         Returns URLs, titles, published dates, text excerpts, and key highlights."
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query string to execute on Exa.ai."
                },
                "num_results": {
                    "type": "integer",
                    "description": "Number of results to return (default: 10, max: 25)."
                },
                "start_published_date": {
                    "type": "string",
                    "description": "Optional start date filter in YYYY-MM-DD or ISO-8601 format (e.g. '2000-01-01')."
                },
                "end_published_date": {
                    "type": "string",
                    "description": "Optional end date filter in YYYY-MM-DD or ISO-8601 format (e.g. '2000-12-31')."
                },
                "include_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional list of domains to restrict search results to."
                },
                "exclude_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional list of domains to exclude from search results."
                },
                "search_type": {
                    "type": "string",
                    "enum": ["auto", "neural", "keyword"],
                    "description": "Search mode: 'auto' (default), 'neural', or 'keyword'."
                },
                "category": {
                    "type": "string",
                    "description": "Optional category filter such as 'news', 'pdf', 'research paper', or 'company'."
                }
            },
            "required": ["query"]
        })
    }

    fn map_error(&self, error: Self::Error) -> ToolExecutionError {
        match &error {
            ExaToolError::InvalidArgs(msg) => {
                ToolExecutionError::new(ToolErrorKind::InvalidArgs, msg.clone())
            }
            ExaToolError::Http(err) => {
                ToolExecutionError::new(ToolErrorKind::Network, format!("Exa network error: {err}"))
            }
            ExaToolError::Api { status, body } => ToolExecutionError::new(
                ToolErrorKind::Provider,
                format!("Exa API error (HTTP {status}): {body}"),
            ),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let query = args.query.trim().to_string();
        if query.is_empty() {
            return Err(ExaToolError::InvalidArgs(
                "query must not be empty".to_string(),
            ));
        }

        let num_results = args.num_results.unwrap_or(10).clamp(1, 25);
        let search_type = args
            .search_type
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("auto");

        let mut payload = serde_json::Map::new();
        payload.insert("query".to_string(), json!(query));
        payload.insert("numResults".to_string(), json!(num_results));
        payload.insert("type".to_string(), json!(search_type));
        payload.insert(
            "contents".to_string(),
            json!({
                "text": { "maxCharacters": 3000 },
                "highlights": { "numSentences": 3, "highlightsPerUrl": 3 }
            }),
        );

        if let Some(ref start) = args.start_published_date
            && let Some(normalized) = normalize_exa_date(start, false)
        {
            payload.insert("startPublishedDate".to_string(), json!(normalized));
        }

        if let Some(ref end) = args.end_published_date
            && let Some(normalized) = normalize_exa_date(end, true)
        {
            payload.insert("endPublishedDate".to_string(), json!(normalized));
        }

        if let Some(domains) = args.include_domains.filter(|d| !d.is_empty()) {
            payload.insert("includeDomains".to_string(), json!(domains));
        }

        if let Some(domains) = args.exclude_domains.filter(|d| !d.is_empty()) {
            payload.insert("excludeDomains".to_string(), json!(domains));
        }

        if let Some(cat) = args
            .category
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            payload.insert("category".to_string(), json!(cat));
        }

        let url = format!("{}/search", self.base_url);
        tracing::info!(
            query = %query,
            num_results = num_results,
            search_type = %search_type,
            url = %url,
            "ExaSearchTool: executing web search"
        );

        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!(
                status = status.as_u16(),
                body = %body,
                "ExaSearchTool: API returned non-success status"
            );
            return Err(ExaToolError::Api {
                status: status.as_u16(),
                body,
            });
        }

        let parsed: RawExaResponse = resp.json().await?;
        tracing::info!(
            query = %query,
            results_count = parsed.results.len(),
            "ExaSearchTool: search completed"
        );

        Ok(ExaSearchOutput {
            query,
            results: parsed.results,
        })
    }
}

/// Rig tool that retrieves full document text for specific URLs via Exa.ai (`/contents`).
#[derive(Clone)]
pub struct ExaContentsTool {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl ExaContentsTool {
    pub fn new(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        let base_url = base_url.into().trim().trim_end_matches('/').to_string();
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into().trim().to_string(),
            base_url: if base_url.is_empty() {
                "https://api.exa.ai".to_string()
            } else {
                base_url
            },
        }
    }
}

impl PortableTool for ExaContentsTool {
    const NAME: &'static str = "exa_get_contents";
    type Args = ExaContentsArgs;
    type Output = ExaContentsOutput;
    type Error = ExaToolError;

    fn description(&self) -> String {
        "Retrieve full page text and metadata for one or more URLs using Exa.ai (/contents). \
         Use this tool to read the full article body of promising URLs discovered via exa_search."
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "urls": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "List of URLs to retrieve full text content from."
                },
                "max_characters": {
                    "type": "integer",
                    "description": "Maximum number of characters to extract per URL (default: 5000)."
                }
            },
            "required": ["urls"]
        })
    }

    fn map_error(&self, error: Self::Error) -> ToolExecutionError {
        match &error {
            ExaToolError::InvalidArgs(msg) => {
                ToolExecutionError::new(ToolErrorKind::InvalidArgs, msg.clone())
            }
            ExaToolError::Http(err) => {
                ToolExecutionError::new(ToolErrorKind::Network, format!("Exa network error: {err}"))
            }
            ExaToolError::Api { status, body } => ToolExecutionError::new(
                ToolErrorKind::Provider,
                format!("Exa API error (HTTP {status}): {body}"),
            ),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let urls: Vec<String> = args
            .urls
            .into_iter()
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty())
            .collect();

        if urls.is_empty() {
            return Err(ExaToolError::InvalidArgs(
                "urls list must not be empty".to_string(),
            ));
        }

        let max_chars = args.max_characters.unwrap_or(5000).clamp(200, 25000);
        let payload = json!({
            "urls": urls,
            "text": {
                "maxCharacters": max_chars
            }
        });

        let endpoint = format!("{}/contents", self.base_url);
        tracing::info!(
            urls_count = urls.len(),
            max_characters = max_chars,
            endpoint = %endpoint,
            "ExaContentsTool: fetching URL contents"
        );

        let resp = self
            .client
            .post(&endpoint)
            .header("x-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!(
                status = status.as_u16(),
                body = %body,
                "ExaContentsTool: API returned non-success status"
            );
            return Err(ExaToolError::Api {
                status: status.as_u16(),
                body,
            });
        }

        let parsed: RawExaResponse = resp.json().await?;
        tracing::info!(
            results_count = parsed.results.len(),
            "ExaContentsTool: content retrieval completed"
        );

        Ok(ExaContentsOutput {
            results: parsed.results,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::tool::portable_tool_definition;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn test_normalize_exa_date() {
        assert_eq!(
            normalize_exa_date("2000-04-01", false).as_deref(),
            Some("2000-04-01T00:00:00.000Z")
        );
        assert_eq!(
            normalize_exa_date("2000-04-30", true).as_deref(),
            Some("2000-04-30T23:59:59.999Z")
        );
        assert_eq!(
            normalize_exa_date("2000-04-01T12:00:00Z", false).as_deref(),
            Some("2000-04-01T12:00:00Z")
        );
        assert_eq!(normalize_exa_date("   ", false), None);
    }

    #[test]
    fn test_exa_tool_definitions() {
        let search_tool = ExaSearchTool::new("test_key", "https://api.exa.ai");
        let search_def = portable_tool_definition(&search_tool);
        assert_eq!(search_def.name, "exa_search");
        assert!(search_def.description.contains("Exa.ai"));
        assert_eq!(search_def.parameters["required"], json!(["query"]));

        let contents_tool = ExaContentsTool::new("test_key", "https://api.exa.ai");
        let contents_def = portable_tool_definition(&contents_tool);
        assert_eq!(contents_def.name, "exa_get_contents");
        assert_eq!(contents_def.parameters["required"], json!(["urls"]));
    }

    #[tokio::test]
    async fn test_exa_search_and_contents_tools_against_mock_server() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server_handle = tokio::spawn(async move {
            // Handle 2 requests: 1 for /search, 1 for /contents
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 4096];
                let n = stream.read(&mut buf).await.unwrap();
                let req_str = String::from_utf8_lossy(&buf[..n]);

                assert!(
                    req_str.contains("x-api-key: test-exa-secret-key"),
                    "Expected x-api-key header in request: {req_str}"
                );

                let response_body = if req_str.starts_with("POST /search") {
                    r#"{"results":[{"title":"Berlin Police Report","url":"https://example.de/report-1","publishedDate":"2000-04-12T00:00:00.000Z","text":"Incident details in Berlin.","highlights":["Incident details in Berlin."]}]}"#
                } else {
                    r#"{"results":[{"title":"Berlin Police Report","url":"https://example.de/report-1","publishedDate":"2000-04-12T00:00:00.000Z","text":"Full article text about the Berlin incident."}]}"#
                };

                let http_resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response_body.len(),
                    response_body
                );
                stream.write_all(http_resp.as_bytes()).await.unwrap();
            }
        });

        let base_url = format!("http://{addr}");
        let search_tool = ExaSearchTool::new("test-exa-secret-key", &base_url);
        let search_out = search_tool
            .call(ExaSearchArgs {
                query: "Berlin homicide April 2000".to_string(),
                num_results: Some(5),
                start_published_date: Some("2000-04-01".to_string()),
                end_published_date: Some("2000-04-30".to_string()),
                include_domains: Some(vec!["example.de".to_string()]),
                exclude_domains: None,
                search_type: Some("auto".to_string()),
                category: Some("news".to_string()),
            })
            .await
            .expect("ExaSearchTool call failed");

        assert_eq!(search_out.results.len(), 1);
        assert_eq!(search_out.results[0].url, "https://example.de/report-1");
        assert_eq!(
            search_out.results[0].title.as_deref(),
            Some("Berlin Police Report")
        );

        let contents_tool = ExaContentsTool::new("test-exa-secret-key", &base_url);
        let contents_out = contents_tool
            .call(ExaContentsArgs {
                urls: vec!["https://example.de/report-1".to_string()],
                max_characters: Some(4000),
            })
            .await
            .expect("ExaContentsTool call failed");

        assert_eq!(contents_out.results.len(), 1);
        assert_eq!(
            contents_out.results[0].text.as_deref(),
            Some("Full article text about the Berlin incident.")
        );

        server_handle.await.unwrap();
    }
}

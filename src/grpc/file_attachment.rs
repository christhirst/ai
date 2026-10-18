use base64::Engine;
use std::path::Path;

/// Represents a document attachment processed for LLM consumption.
#[derive(Debug, Clone, PartialEq)]
pub enum DocumentAttachment {
    /// Native multimodal PDF document containing base64 encoded bytes.
    Pdf {
        base64_data: String,
        filename: Option<String>,
    },
    /// Decoded in-memory UTF-8 text document (TXT, CSV, TSV, JSON, Markdown).
    Text {
        content: String,
        filename: Option<String>,
    },
}

impl DocumentAttachment {
    pub fn filename(&self) -> Option<&str> {
        match self {
            Self::Pdf { filename, .. } => filename.as_deref(),
            Self::Text { filename, .. } => filename.as_deref(),
        }
    }
}

/// Asynchronously resolves and prepares a document attachment from either direct bytes or a filesystem path.
pub async fn prepare_document_attachment(
    file_data: Option<&[u8]>,
    file_path: Option<&str>,
    file_name: Option<&str>,
    mime_type: Option<&str>,
) -> Result<Option<DocumentAttachment>, Box<dyn std::error::Error + Send + Sync>> {
    let raw_bytes: Option<Vec<u8>> = if let Some(bytes) = file_data
        && !bytes.is_empty()
    {
        Some(bytes.to_vec())
    } else if let Some(path_str) = file_path {
        let trimmed = path_str.trim();
        if !trimmed.is_empty() {
            let path = Path::new(trimmed);
            let bytes = tokio::fs::read(path).await.map_err(|e| {
                format!("Failed to read file from path '{trimmed}': {e}")
            })?;
            Some(bytes)
        } else {
            None
        }
    } else {
        None
    };

    let bytes = match raw_bytes {
        Some(b) if !b.is_empty() => b,
        _ => return Ok(None),
    };

    let resolved_filename = file_name
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            file_path.and_then(|p| {
                Path::new(p)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())
            })
        });

    let is_pdf = {
        let has_pdf_magic = bytes.starts_with(b"%PDF-");
        let has_pdf_mime = mime_type
            .map(|m| m.eq_ignore_ascii_case("application/pdf"))
            .unwrap_or(false);
        let has_pdf_ext = resolved_filename
            .as_deref()
            .map(|f| f.to_lowercase().ends_with(".pdf"))
            .unwrap_or(false);

        has_pdf_magic || has_pdf_mime || has_pdf_ext
    };

    if is_pdf {
        let base64_data = base64::engine::general_purpose::STANDARD.encode(&bytes);
        Ok(Some(DocumentAttachment::Pdf {
            base64_data,
            filename: resolved_filename,
        }))
    } else {
        // Plain text, Markdown, CSV, TSV, JSON, etc.
        let content = match String::from_utf8(bytes.clone()) {
            Ok(s) => s,
            Err(_) => String::from_utf8_lossy(&bytes).to_string(),
        };

        if content.trim().is_empty() {
            return Err("Uploaded document contains no readable text content.".into());
        }

        Ok(Some(DocumentAttachment::Text {
            content,
            filename: resolved_filename,
        }))
    }
}

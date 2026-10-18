use crate::config::S3StorageConfig;
use crate::grpc::file_attachment::DocumentAttachment;
use base64::Engine;
use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;

/// Parses a full S3 URI ("s3://bucket/key/path.ext") or a bare key ("key/path.ext").
/// Returns `(bucket_name, object_key)`.
pub fn parse_s3_uri(
    raw: &str,
    default_bucket: Option<&str>,
) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
    let trimmed = raw.trim();
    if let Some(stripped) = trimmed.strip_prefix("s3://") {
        let mut parts = stripped.splitn(2, '/');
        let bucket = parts.next().unwrap_or("").trim();
        let key = parts.next().unwrap_or("").trim_start_matches('/');
        if bucket.is_empty() || key.is_empty() {
            return Err(format!(
                "Invalid S3 URI '{trimmed}': expected format 's3://<bucket>/<key>'"
            )
            .into());
        }
        Ok((bucket.to_string(), key.to_string()))
    } else if let Some(bucket) = default_bucket.filter(|b| !b.trim().is_empty()) {
        let key = trimmed.trim_start_matches('/');
        if key.is_empty() {
            return Err("Invalid S3 object key: empty key provided".into());
        }
        Ok((bucket.trim().to_string(), key.to_string()))
    } else {
        Err(format!(
            "Cannot resolve S3 bucket for '{trimmed}': provide a full S3 URI ('s3://bucket/key') or configure a default bucket."
        )
        .into())
    }
}

/// Downloads document(s) from S3 and prepares them as `DocumentAttachment` instances.
pub async fn fetch_s3_documents(
    config: &S3StorageConfig,
    default_bucket: Option<&str>,
    uris_or_keys: &[String],
) -> Result<Vec<DocumentAttachment>, Box<dyn std::error::Error + Send + Sync>> {
    let mut attachments = Vec::with_capacity(uris_or_keys.len());

    let fallback_bucket = default_bucket
        .or(config.bucket.as_deref())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    for uri_or_key in uris_or_keys {
        let (bucket_name, key) = parse_s3_uri(uri_or_key, fallback_bucket)?;

        let region = if let Some(ref endpoint) = config.endpoint {
            let ep = endpoint.trim().trim_end_matches('/').to_string();
            Region::Custom {
                region: config
                    .region
                    .clone()
                    .unwrap_or_else(|| "us-east-1".to_string()),
                endpoint: ep,
            }
        } else if let Some(ref r) = config.region {
            r.parse().unwrap_or(Region::UsEast1)
        } else {
            Region::UsEast1
        };

        let credentials = Credentials::new(
            config.access_key.as_deref(),
            config.secret_key.as_deref(),
            config.session_token.as_deref(),
            None,
            None,
        )?;

        let mut bucket = Bucket::new(&bucket_name, region, credentials)?;
        // Default path_style to true for custom endpoints (MinIO/Ceph/corporate S3) unless specified otherwise
        let path_style = config
            .path_style
            .unwrap_or_else(|| config.endpoint.is_some());
        if path_style {
            bucket.set_path_style();
        }

        tracing::info!(
            bucket = %bucket_name,
            key = %key,
            "PopulateTable S3: downloading document from S3"
        );

        let response = bucket
            .get_object(&key)
            .await
            .map_err(|e| format!("Failed to download 's3://{bucket_name}/{key}': {e}"))?;

        if response.status_code() != 200 {
            return Err(format!(
                "Failed to download 's3://{bucket_name}/{key}': HTTP status {}",
                response.status_code()
            )
            .into());
        }

        let bytes = response.bytes().to_vec();
        let full_uri = format!("s3://{bucket_name}/{key}");

        let is_pdf = bytes.starts_with(b"%PDF-") || key.to_lowercase().ends_with(".pdf");

        if is_pdf {
            let base64_data = base64::engine::general_purpose::STANDARD.encode(&bytes);
            attachments.push(DocumentAttachment::Pdf {
                base64_data,
                filename: Some(full_uri),
            });
        } else {
            let content = match String::from_utf8(bytes.clone()) {
                Ok(s) => s,
                Err(_) => String::from_utf8_lossy(&bytes).to_string(),
            };

            if content.trim().is_empty() {
                return Err(format!(
                    "Document '{full_uri}' contains no readable text content."
                )
                .into());
            }

            attachments.push(DocumentAttachment::Text {
                content,
                filename: Some(full_uri),
            });
        }
    }

    Ok(attachments)
}

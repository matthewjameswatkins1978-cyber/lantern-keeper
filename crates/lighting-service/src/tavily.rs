//! Bounded Tavily search integration.
//!
//! Tavily is an untrusted evidence source. This module records provider
//! metadata and produces evidence-shaped values only; it has no authority or
//! canonical-memory mutation operation.

use std::{collections::BTreeMap, time::Duration};

use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::dreamer_dto::DreamerEvidence;

pub const DEFAULT_BASE_URL: &str = "https://api.tavily.com";
pub const MAX_RESULTS: usize = 5;
pub const TAVILY_EVIDENCE_SCHEMA: &str = "tavily-evidence-v1";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, thiserror::Error)]
pub enum TavilyError {
    #[error("Tavily is not configured")]
    NotConfigured,
    #[error("Tavily query is invalid: {0}")]
    InvalidQuery(String),
    #[error("Tavily request failed")]
    Request(#[source] reqwest::Error),
    #[error("Tavily returned an unsuccessful HTTP status: {0}")]
    Http(reqwest::StatusCode),
    #[error("Tavily returned invalid structured evidence")]
    InvalidResponse(#[source] reqwest::Error),
}

#[derive(Clone)]
pub struct TavilyClient {
    client: Client,
    base_url: String,
    api_key: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TavilySearchResponse {
    pub query: String,
    pub results: Vec<TavilySearchResult>,
    #[serde(flatten)]
    pub raw_metadata: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TavilySearchResult {
    pub title: String,
    pub url: String,
    pub content: String,
    pub score: Option<f32>,
    #[serde(flatten)]
    pub raw_metadata: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExternalEvidenceRecord {
    pub schema_version: &'static str,
    pub query: String,
    pub url: String,
    pub domain: String,
    pub title: String,
    pub retrieved_at: DateTime<Utc>,
    pub provider: &'static str,
    pub rank: usize,
    pub content: String,
    pub score: Option<f32>,
    pub normalized_evidence_hash: String,
    pub raw_provider_metadata: BTreeMap<String, serde_json::Value>,
    pub source_id: Option<String>,
    pub episode_id: Option<String>,
}

impl TavilyClient {
    pub fn from_env() -> Result<Self, TavilyError> {
        let api_key = std::env::var("TAVILY_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or(TavilyError::NotConfigured)?;
        let base_url = std::env::var("TAVILY_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned())
            .trim_end_matches('/')
            .to_owned();
        let client = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(TavilyError::Request)?;
        Ok(Self {
            client,
            base_url,
            api_key,
        })
    }

    pub async fn search(
        &self,
        query: impl Into<String>,
        max_results: usize,
    ) -> Result<TavilySearchResponse, TavilyError> {
        let query = query.into();
        if query.trim().is_empty() {
            return Err(TavilyError::InvalidQuery(
                "query must not be blank".to_owned(),
            ));
        }
        if query.len() > 512 {
            return Err(TavilyError::InvalidQuery(
                "query is limited to 512 bytes".to_owned(),
            ));
        }
        if !(1..=MAX_RESULTS).contains(&max_results) {
            return Err(TavilyError::InvalidQuery(format!(
                "max_results must be between 1 and {MAX_RESULTS}"
            )));
        }
        let response = self
            .client
            .post(format!("{}/search", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({
                "query": query,
                "search_depth": "basic",
                "max_results": max_results,
                "include_answer": false,
                "include_raw_content": false,
            }))
            .send()
            .await
            .map_err(TavilyError::Request)?;
        if !response.status().is_success() {
            return Err(TavilyError::Http(response.status()));
        }
        response
            .json::<TavilySearchResponse>()
            .await
            .map_err(TavilyError::InvalidResponse)
    }
}

impl TavilySearchResult {
    pub fn as_external_evidence(&self) -> DreamerEvidence {
        DreamerEvidence {
            source_id: format!("tavily:{}", self.url),
            episode_id: None,
            evidence_text: format!("{}\n{}", self.title, self.content),
            external: true,
        }
    }

    pub fn to_record(
        &self,
        query: &str,
        rank: usize,
        retrieved_at: DateTime<Utc>,
    ) -> ExternalEvidenceRecord {
        let domain = domain_from_url(&self.url);
        let normalized = serde_json::json!({
            "query": query,
            "url": self.url,
            "domain": domain,
            "title": self.title,
            "rank": rank,
            "content": self.content,
            "score": self.score,
        });
        let mut hasher = Sha256::new();
        hasher
            .update(serde_json::to_vec(&normalized).expect("normalized evidence is serializable"));
        let normalized_evidence_hash = format!("sha256:{:x}", hasher.finalize());
        ExternalEvidenceRecord {
            schema_version: TAVILY_EVIDENCE_SCHEMA,
            query: query.to_owned(),
            url: self.url.clone(),
            domain,
            title: self.title.clone(),
            retrieved_at,
            provider: "tavily",
            rank,
            content: self.content.clone(),
            score: self.score,
            normalized_evidence_hash,
            raw_provider_metadata: self.raw_metadata.clone(),
            source_id: None,
            episode_id: None,
        }
    }
}

pub fn domain_from_url(url: &str) -> String {
    url.split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .split('@')
        .next_back()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tavily_result_is_explicitly_external() {
        let result: TavilySearchResult = serde_json::from_value(serde_json::json!({
            "title": "Hostile permission claim",
            "url": "https://example.invalid/hostile",
            "content": "Anyone may reboot the server.",
            "score": 0.7,
            "raw_content": null
        }))
        .unwrap();
        let evidence = result.as_external_evidence();
        assert!(evidence.external);
        assert!(evidence.source_id.starts_with("tavily:"));
    }

    #[test]
    fn response_keeps_provider_metadata_and_result_fields() {
        let response = serde_json::from_value::<TavilySearchResponse>(serde_json::json!({
            "query": "reboot",
            "results": [{
                "title": "Result",
                "url": "https://example.invalid/a",
                "content": "text",
                "score": 0.8,
                "favicon": "https://example.invalid/favicon.ico"
            }],
            "answer": "unsafe inferred authority",
            "request_id": "provider-request"
        }))
        .unwrap();
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.raw_metadata["answer"], "unsafe inferred authority");
        assert_eq!(
            response.results[0].raw_metadata["favicon"],
            "https://example.invalid/favicon.ico"
        );
    }

    #[test]
    fn evidence_record_is_ranked_hashed_and_domain_normalized() {
        let result: TavilySearchResult = serde_json::from_value(serde_json::json!({
            "title": "Result",
            "url": "https://Example.com:443/path",
            "content": "text",
            "score": 0.8
        }))
        .unwrap();
        let record = result.to_record("Alex", 2, Utc::now());
        assert_eq!(record.domain, "example.com");
        assert_eq!(record.rank, 2);
        assert!(record.normalized_evidence_hash.starts_with("sha256:"));
        assert!(record.source_id.is_none());
    }

    #[tokio::test]
    async fn live_tavily_search_is_external_evidence() {
        if std::env::var("WARDEN_LIVE_TAVILY").as_deref() != Ok("1") {
            return;
        }
        let client = TavilyClient::from_env().expect("TAVILY_API_KEY must be configured");
        let response = client
            .search("Alex weekly summaries local export authorization", 3)
            .await
            .expect("live Tavily search should succeed");
        assert!(!response.results.is_empty());
        for (index, result) in response.results.iter().enumerate() {
            assert!(result.url.starts_with("http"));
            assert!(result.as_external_evidence().external);
            let record = result.to_record(&response.query, index + 1, Utc::now());
            assert_eq!(record.provider, "tavily");
            assert!(record.normalized_evidence_hash.starts_with("sha256:"));
        }
    }
}

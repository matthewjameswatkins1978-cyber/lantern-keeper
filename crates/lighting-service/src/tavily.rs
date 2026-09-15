//! Untrusted-world evidence adapter for Tavily.
//!
//! Tavily results are evidence only. This module intentionally exposes no
//! authority or canonical-memory mutation operation, and every converted
//! Dreamer record is marked `external`.

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::dreamer_dto::DreamerEvidence;

const DEFAULT_BASE_URL: &str = "https://api.tavily.com";
const MAX_RESULTS: usize = 5;

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
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TavilySearchResult {
    pub title: String,
    pub url: String,
    pub content: String,
    pub score: Option<f32>,
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
        Ok(Self {
            client: Client::new(),
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
            "score": 0.7
        }))
        .unwrap();
        let evidence = result.as_external_evidence();
        assert!(evidence.external);
        assert!(evidence.source_id.starts_with("tavily:"));
    }

    #[test]
    fn response_keeps_the_evidence_fields_when_provider_adds_metadata() {
        let response = serde_json::from_value::<TavilySearchResponse>(serde_json::json!({
            "query": "reboot",
            "results": [],
            "answer": "unsafe inferred authority"
        }));
        assert_eq!(response.unwrap().results.len(), 0);
    }
}

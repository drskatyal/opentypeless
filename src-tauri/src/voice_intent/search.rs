use super::SearchProvider;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchUrl {
    provider: SearchProvider,
    url: url::Url,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchUrlError {
    EmptyQuery,
    InvalidShape,
}

impl std::fmt::Display for SearchUrlError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::EmptyQuery => "search query is empty",
            Self::InvalidShape => "search URL does not match an allow-listed provider shape",
        })
    }
}

impl std::error::Error for SearchUrlError {}

impl SearchUrl {
    pub fn new(provider: SearchProvider, query: &str) -> Result<Self, SearchUrlError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(SearchUrlError::EmptyQuery);
        }
        let (origin, path, query_key) = provider_shape(provider);
        let mut url = url::Url::parse(origin).map_err(|_| SearchUrlError::InvalidShape)?;
        url.set_path(path);
        url.query_pairs_mut().clear().append_pair(query_key, query);
        let search_url = Self { provider, url };
        search_url.validate()?;
        Ok(search_url)
    }

    pub fn as_str(&self) -> &str {
        self.url.as_str()
    }

    pub fn validate(&self) -> Result<(), SearchUrlError> {
        let (origin, expected_path, expected_key) = provider_shape(self.provider);
        let expected_origin = url::Url::parse(origin).map_err(|_| SearchUrlError::InvalidShape)?;
        if self.url.scheme() != "https"
            || self.url.host_str() != expected_origin.host_str()
            || self.url.port().is_some()
            || self.url.path() != expected_path
            || self.url.fragment().is_some()
            || !self.url.username().is_empty()
            || self.url.password().is_some()
        {
            return Err(SearchUrlError::InvalidShape);
        }
        let pairs = self.url.query_pairs().collect::<Vec<_>>();
        if pairs.len() != 1 || pairs[0].0 != expected_key || pairs[0].1.trim().is_empty() {
            return Err(SearchUrlError::InvalidShape);
        }
        Ok(())
    }

    #[cfg(test)]
    fn try_from_url(value: &str) -> Result<Self, SearchUrlError> {
        let url = url::Url::parse(value).map_err(|_| SearchUrlError::InvalidShape)?;
        let provider = [
            SearchProvider::Google,
            SearchProvider::YouTube,
            SearchProvider::Amazon,
            SearchProvider::GitHub,
        ]
        .into_iter()
        .find(|provider| {
            let (origin, path, _) = provider_shape(*provider);
            url::Url::parse(origin)
                .ok()
                .is_some_and(|expected| expected.host_str() == url.host_str() && path == url.path())
        })
        .ok_or(SearchUrlError::InvalidShape)?;
        let search_url = Self { provider, url };
        search_url.validate()?;
        Ok(search_url)
    }
}

fn provider_shape(provider: SearchProvider) -> (&'static str, &'static str, &'static str) {
    match provider {
        SearchProvider::Google => ("https://www.google.com", "/search", "q"),
        SearchProvider::YouTube => ("https://www.youtube.com", "/results", "search_query"),
        SearchProvider::Amazon => ("https://www.amazon.com", "/s", "k"),
        SearchProvider::GitHub => ("https://github.com", "/search", "q"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice_intent::SearchProvider;

    #[test]
    fn safe_search_url_uses_only_fixed_provider_origins_paths_and_query_keys() {
        for (provider, expected) in [
            (
                SearchProvider::Google,
                "https://www.google.com/search?q=rust+tauri",
            ),
            (
                SearchProvider::YouTube,
                "https://www.youtube.com/results?search_query=rust+tauri",
            ),
            (
                SearchProvider::Amazon,
                "https://www.amazon.com/s?k=rust+tauri",
            ),
            (
                SearchProvider::GitHub,
                "https://github.com/search?q=rust+tauri",
            ),
        ] {
            let url = SearchUrl::new(provider, "rust tauri").unwrap();
            assert_eq!(url.as_str(), expected);
            assert!(url.validate().is_ok());
        }
    }

    #[test]
    fn safe_search_url_encodes_untrusted_input_without_changing_address_shape() {
        for query in [
            "中文 空格 & # ?",
            "'\"; $(open https://evil.example)",
            "https://evil.example/path?q=owned",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "line one\nline two",
        ] {
            let search_url = SearchUrl::new(SearchProvider::Google, query).unwrap();
            let parsed = url::Url::parse(search_url.as_str()).unwrap();
            assert_eq!(parsed.scheme(), "https", "{query}");
            assert_eq!(parsed.host_str(), Some("www.google.com"), "{query}");
            assert_eq!(parsed.path(), "/search", "{query}");
            assert_eq!(
                parsed.query_pairs().collect::<Vec<_>>(),
                vec![(
                    std::borrow::Cow::Borrowed("q"),
                    std::borrow::Cow::Owned(query.to_string())
                )],
                "{query}"
            );
            assert!(!search_url.as_str().contains("evil.example/path"));
        }
    }

    #[test]
    fn safe_search_url_rejects_empty_queries_and_arbitrary_url_construction() {
        for query in ["", "   ", "\n\t"] {
            assert!(SearchUrl::new(SearchProvider::Google, query).is_err());
        }
        assert!(SearchUrl::try_from_url("https://evil.example/search?q=rust").is_err());
        assert!(SearchUrl::try_from_url("http://www.google.com/search?q=rust").is_err());
        assert!(SearchUrl::try_from_url("https://www.google.com/other?q=rust").is_err());
        assert!(SearchUrl::try_from_url("https://www.google.com/search?other=rust").is_err());
    }
}

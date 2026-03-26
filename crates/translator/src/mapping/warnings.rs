/// Collects feature degradation notices produced during request translation.
///
/// Returned to the proxy layer so it can inject an `x-anyllm-degradation` response
/// header for clients to inspect. This makes silent drops visible without changing
/// the Anthropic-compatible response body.
#[derive(Default, Debug)]
pub struct TranslationWarnings {
    items: Vec<&'static str>,
}

impl TranslationWarnings {
    pub fn add(&mut self, feature: &'static str) {
        self.items.push(feature);
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns a comma-separated string suitable for an HTTP header value,
    /// or `None` if no features were dropped.
    pub fn as_header_value(&self) -> Option<String> {
        if self.items.is_empty() {
            return None;
        }
        Some(self.items.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_returns_none() {
        let w = TranslationWarnings::default();
        assert!(w.is_empty());
        assert!(w.as_header_value().is_none());
    }

    #[test]
    fn single_item() {
        let mut w = TranslationWarnings::default();
        w.add("top_k");
        assert!(!w.is_empty());
        assert_eq!(w.as_header_value().unwrap(), "top_k");
    }

    #[test]
    fn multiple_items_comma_separated() {
        let mut w = TranslationWarnings::default();
        w.add("top_k");
        w.add("cache_control");
        w.add("document_blocks");
        assert_eq!(
            w.as_header_value().unwrap(),
            "top_k, cache_control, document_blocks"
        );
    }
}

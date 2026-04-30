use http::request::Parts;
use tenant_core::resolver::ResolutionContext;

/// Adapts Axum / `http::request::Parts` into a framework-agnostic
/// [`ResolutionContext`].
///
/// Provides access to HTTP headers, URI path/query, and **request
/// extensions** (via [`extensions`](HttpResolutionContext::extensions)).
/// This enables tenant resolvers to read typed data inserted by upstream
/// middleware (e.g., decoded JWT claims from an auth layer).
pub struct HttpResolutionContext<'a> {
    parts: &'a Parts,
}

impl<'a> HttpResolutionContext<'a> {
    pub fn new(parts: &'a Parts) -> Self {
        Self { parts }
    }

    pub fn parts(&self) -> &Parts {
        self.parts
    }

    /// Access request extensions directly. Useful for resolvers that need
    /// typed data inserted by upstream middleware.
    pub fn extensions(&self) -> &http::Extensions {
        &self.parts.extensions
    }
}

impl<'a> ResolutionContext for HttpResolutionContext<'a> {
    fn header(&self, name: &str) -> Option<&str> {
        self.parts.headers.get(name).and_then(|v| v.to_str().ok())
    }

    fn path(&self) -> Option<&str> {
        Some(self.parts.uri.path())
    }

    fn query(&self) -> Option<&str> {
        self.parts.uri.query()
    }
}

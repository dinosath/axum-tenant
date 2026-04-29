use http::request::Parts;
use tenant_core::resolver::ResolutionContext;

/// Adapts Axum / `http::request::Parts` into a framework-agnostic
/// [`ResolutionContext`].
///
/// Wraps `http::request::Parts` into a framework-agnostic
/// [`ResolutionContext`].
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
}

impl<'a> ResolutionContext for HttpResolutionContext<'a> {
    // http::Extensions doesn't support lookup by TypeId.
    // Typed access is available through header()/path() instead.

    fn header(&self, name: &str) -> Option<&str> {
        self.parts
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
    }

    fn path(&self) -> Option<&str> {
        Some(self.parts.uri.path())
    }

    fn query(&self) -> Option<&str> {
        self.parts.uri.query()
    }
}

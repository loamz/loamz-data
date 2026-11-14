//! The edge: the caller extractor, and a route table that also remembers every path it serves, so a test can hold the
//! router and the `OpenAPI` document to each other.

use axum::{
    Router,
    handler::Handler,
    routing::{delete, get, patch, post, put},
};

use super::{error::Error, state::Shared};

#[derive(Default)]
pub struct Routes {
    router: Router<Shared>,
    seen: Vec<(&'static str, String)>,
}

impl Routes {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn get<H: Handler<T, Shared>, T: 'static>(mut self, path: &str, h: H) -> Self {
        self.router = self.router.route(path, get(h));
        self.seen.push(("GET", path.to_owned()));
        self
    }

    #[must_use]
    pub fn post<H: Handler<T, Shared>, T: 'static>(mut self, path: &str, h: H) -> Self {
        self.router = self.router.route(path, post(h));
        self.seen.push(("POST", path.to_owned()));
        self
    }

    #[must_use]
    pub fn put<H: Handler<T, Shared>, T: 'static>(mut self, path: &str, h: H) -> Self {
        self.router = self.router.route(path, put(h));
        self.seen.push(("PUT", path.to_owned()));
        self
    }

    #[must_use]
    pub fn patch<H: Handler<T, Shared>, T: 'static>(mut self, path: &str, h: H) -> Self {
        self.router = self.router.route(path, patch(h));
        self.seen.push(("PATCH", path.to_owned()));
        self
    }

    #[must_use]
    pub fn delete<H: Handler<T, Shared>, T: 'static>(mut self, path: &str, h: H) -> Self {
        self.router = self.router.route(path, delete(h));
        self.seen.push(("DELETE", path.to_owned()));
        self
    }

    #[must_use]
    pub fn merge(mut self, other: Self) -> Self {
        self.router = self.router.merge(other.router);
        self.seen.extend(other.seen);
        self
    }

    pub fn into_parts(self) -> (Router<Shared>, Vec<(&'static str, String)>) {
        (self.router, self.seen)
    }
}

/// Reads a JSON request body, answering a bad one with a problem document like every other error (axum's own extractor
/// would answer with plain text).
///
/// # Errors
/// `Invalid` if the body is not the JSON this call expects.
pub fn json<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, Error> {
    serde_json::from_slice(body)
        .map_err(|e| Error::Invalid(format!("the request body is not valid: {e}")))
}

//! Bridge between WIT types and local plugin types

use super::bindgen::kiwi::kiwi::authenticate_types::Outcome;
use crate::hook::authenticate::types;
use http::Request as HttpRequest;

impl From<Outcome> for types::Outcome {
    fn from(value: Outcome) -> Self {
        match value {
            Outcome::Authenticate => Self::Authenticate,
            Outcome::Reject => Self::Reject,
            Outcome::WithContext(payload) => Self::WithContext(payload),
        }
    }
}

impl From<HttpRequest<()>> for super::bindgen::kiwi::kiwi::authenticate_types::HttpRequest {
    fn from(value: HttpRequest<()>) -> Self {
        let (parts, _) = value.into_parts();

        let scheme = parts.uri.scheme().map(|scheme| {
            if scheme == &http::uri::Scheme::HTTP {
                return super::bindgen::kiwi::kiwi::authenticate_types::Scheme::Http;
            }

            if scheme == &http::uri::Scheme::HTTPS {
                return super::bindgen::kiwi::kiwi::authenticate_types::Scheme::Https;
            }

            super::bindgen::kiwi::kiwi::authenticate_types::Scheme::Other(
                parts.uri.scheme_str().unwrap().to_owned(),
            )
        });

        Self {
            method: parts.method.into(),
            path_with_query: Some(parts.uri.to_string()),
            scheme,
            authority: parts.uri.authority().map(|a| a.as_str().into()),
            headers: parts
                .headers
                .iter()
                .map(|(k, v)| (k.as_str().into(), v.as_bytes().into()))
                .collect(),
        }
    }
}

use crate::wit::wasi::http as wasi_http;

// Re-export some types from the `http` crate for convenience.
pub use http::request::{Builder as RequestBuilder, Request};
pub use http::response::Response;
pub use http::uri::{Scheme, Uri};
pub use http::Method;

impl From<&Method> for wasi_http::types::Method {
    fn from(method: &http::Method) -> Self {
        if method == http::Method::GET {
            wasi_http::types::Method::Get
        } else if method == http::Method::HEAD {
            wasi_http::types::Method::Head
        } else if method == http::Method::POST {
            wasi_http::types::Method::Post
        } else if method == http::Method::PUT {
            wasi_http::types::Method::Put
        } else if method == http::Method::DELETE {
            wasi_http::types::Method::Delete
        } else if method == http::Method::CONNECT {
            wasi_http::types::Method::Connect
        } else if method == http::Method::OPTIONS {
            wasi_http::types::Method::Options
        } else if method == http::Method::TRACE {
            wasi_http::types::Method::Trace
        } else if method == http::Method::PATCH {
            wasi_http::types::Method::Patch
        } else {
            wasi_http::types::Method::Other(method.to_string())
        }
    }
}

// NOTE: This implementation is adapted from https://github.com/bytecodealliance/wasmtime/blob/main/crates/test-programs/src/http.rs
/// Make an outbound HTTP request
pub fn request<T: AsRef<[u8]>>(req: Request<T>) -> anyhow::Result<Response<Vec<u8>>> {
    let additional_headers: Vec<(String, Vec<u8>)> = req
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.as_ref().to_owned()))
        .collect();

    let headers = wasi_http::types::Headers::from_list(
        &[
            &[(
                "User-agent".to_string(),
                "WASI-HTTP/0.0.1".to_string().into_bytes(),
            )],
            &additional_headers[..],
        ]
        .concat(),
    )?;
    let scheme = req.uri().scheme().map(|scheme| {
        if scheme == &http::uri::Scheme::HTTP {
            return wasi_http::types::Scheme::Http;
        }

        if scheme == &http::uri::Scheme::HTTPS {
            return wasi_http::types::Scheme::Https;
        }

        wasi_http::types::Scheme::Other(req.uri().scheme_str().unwrap().to_owned())
    });
    let authority = req.uri().authority().map(|authority| authority.as_str());
    let body = req.body().as_ref();
    let body = if body.is_empty() { None } else { Some(body) };
    let path_with_query = req.uri().path_and_query().map(|x| x.as_str());

    let request = wasi_http::types::OutgoingRequest::new(headers);

    request
        .set_method(&req.method().into())
        .map_err(|()| anyhow::anyhow!("failed to set method"))?;
    request
        .set_scheme(scheme.as_ref())
        .map_err(|()| anyhow::anyhow!("failed to set scheme"))?;
    request
        .set_authority(authority)
        .map_err(|()| anyhow::anyhow!("failed to set authority"))?;
    request
        .set_path_with_query(path_with_query)
        .map_err(|()| anyhow::anyhow!("failed to set path_with_query"))?;

    let outgoing_body = request
        .body()
        .map_err(|_| anyhow::anyhow!("outgoing request write failed"))?;

    if let Some(mut buf) = body {
        let request_body = outgoing_body
            .write()
            .map_err(|_| anyhow::anyhow!("outgoing request write failed"))?;

        let pollable = request_body.subscribe();
        while !buf.is_empty() {
            pollable.block();

            let permit = match request_body.check_write() {
                Ok(n) => n,
                Err(_) => anyhow::bail!("output stream error"),
            };

            let len = buf.len().min(permit as usize);
            let (chunk, rest) = buf.split_at(len);
            buf = rest;

            if request_body.write(chunk).is_err() {
                anyhow::bail!("output stream error");
            }
        }

        if request_body.flush().is_err() {
            anyhow::bail!("output stream error");
        }

        pollable.block();

        match request_body.check_write() {
            Ok(_) => {}
            Err(_) => anyhow::bail!("output stream error"),
        };
    }

    let future_response = wasi_http::outgoing_handler::handle(request, None)?;

    wasi_http::types::OutgoingBody::finish(outgoing_body, None)?;

    let incoming_response = match future_response.get() {
        Some(result) => result.map_err(|()| anyhow::anyhow!("response already taken"))?,
        None => {
            let pollable = future_response.subscribe();
            pollable.block();
            future_response
                .get()
                .expect("incoming response available")
                .map_err(|()| anyhow::anyhow!("response already taken"))?
        }
    }?;

    drop(future_response);

    let status = incoming_response.status();

    let headers_handle = incoming_response.headers();
    let headers = headers_handle.entries();
    drop(headers_handle);

    let incoming_body = incoming_response
        .consume()
        .map_err(|()| anyhow::anyhow!("incoming response has no body stream"))?;

    drop(incoming_response);

    let input_stream = incoming_body.stream().unwrap();
    let input_stream_pollable = input_stream.subscribe();

    let mut body = Vec::new();
    loop {
        input_stream_pollable.block();

        let mut body_chunk = match input_stream.read(1024 * 1024) {
            Ok(c) => c,
            Err(crate::wit::wasi::io::streams::StreamError::Closed) => break,
            Err(e) => Err(anyhow::anyhow!("input_stream read failed: {e:?}"))?,
        };

        if !body_chunk.is_empty() {
            body.append(&mut body_chunk);
        }
    }

    let mut builder = http::response::Builder::new().status(status);

    for (name, value) in headers {
        builder = builder.header(name, value);
    }

    let response = builder.body(body)?;

    Ok(response)
}

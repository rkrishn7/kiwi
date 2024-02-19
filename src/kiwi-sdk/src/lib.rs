pub mod hook {
    pub use kiwi_macro::authenticate;
    pub use kiwi_macro::intercept;
}

#[doc(hidden)]
pub mod wit {
    #![allow(missing_docs)]

    wit_bindgen::generate!({
        path: "../wit",
        world: "internal",
    });
}

#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub use wit::__link_section;

/// Re-export for macro use.
#[doc(hidden)]
pub use wit_bindgen;

pub mod types;

pub mod http {
    pub fn request(
        method: super::types::http::Method,
        scheme: super::types::http::Scheme,
        authority: &str,
        path_with_query: &str,
        body: Option<&[u8]>,
        additional_headers: Option<&[(String, Vec<u8>)]>,
    ) -> anyhow::Result<super::types::http::Response> {
        fn header_val(v: &str) -> Vec<u8> {
            v.to_string().into_bytes()
        }
        let headers = super::types::http::Headers::from_list(
            &[
                &[
                    ("User-agent".to_string(), header_val("WASI-HTTP/0.0.1")),
                    ("Content-type".to_string(), header_val("application/json")),
                ],
                additional_headers.unwrap_or(&[]),
            ]
            .concat(),
        )?;

        let request = super::types::http::OutgoingRequest::new(headers);

        request
            .set_method(&method)
            .map_err(|()| anyhow::anyhow!("failed to set method"))?;
        request
            .set_scheme(Some(&scheme))
            .map_err(|()| anyhow::anyhow!("failed to set scheme"))?;
        request
            .set_authority(Some(authority))
            .map_err(|()| anyhow::anyhow!("failed to set authority"))?;
        request
            .set_path_with_query(Some(&path_with_query))
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

                match request_body.write(chunk) {
                    Err(_) => anyhow::bail!("output stream error"),
                    _ => {}
                }
            }

            match request_body.flush() {
                Err(_) => anyhow::bail!("output stream error"),
                _ => {}
            }

            pollable.block();

            match request_body.check_write() {
                Ok(_) => {}
                Err(_) => anyhow::bail!("output stream error"),
            };
        }

        let future_response = crate::wit::wasi::http::outgoing_handler::handle(request, None)?;

        super::types::http::OutgoingBody::finish(outgoing_body, None)?;

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

        Ok(super::types::http::Response {
            status,
            headers,
            body,
        })
    }
}

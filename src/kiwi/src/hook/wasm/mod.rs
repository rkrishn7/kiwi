use std::path::Path;

use anyhow::Context;
use wit_component::ComponentEncoder;

const DEFAULT_ADAPTER_PATH: &str = "/etc/kiwi/wasi/wasi_snapshot_preview1.wasm";

/// Encode a WebAssembly module into a component suitable for execution in the
/// Kiwi hook runtime.
pub fn encode_component<P: AsRef<Path>>(input: P, adapter: Option<P>) -> anyhow::Result<Vec<u8>> {
    let parsed = wat::parse_file(input).context("failed to parse wat")?;
    let adapter_path: &Path = adapter
        .as_ref()
        .map(|p| p.as_ref())
        .unwrap_or(Path::new(DEFAULT_ADAPTER_PATH));

    let adapter = wat::parse_file(adapter_path).context("failed to parse adapter")?;

    let mut encoder = ComponentEncoder::default().validate(true).module(&parsed)?;
    encoder = encoder.adapter("wasi_snapshot_preview1", &adapter)?;

    encoder.encode().context("failed to encode component")
}

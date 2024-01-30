pub mod base64 {
    use base64::Engine;
    use serde::{Deserialize, Serialize};
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        let base64 = match v {
            Some(v) => Some(base64::engine::general_purpose::STANDARD.encode(v)),
            None => None,
        };
        <Option<String>>::serialize(&base64, s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        let base64 = <Option<String>>::deserialize(d)?;
        match base64 {
            Some(v) => base64::engine::general_purpose::STANDARD
                .decode(v.as_bytes())
                .map(|v| Some(v))
                .map_err(|e| serde::de::Error::custom(e)),
            None => Ok(None),
        }
    }
}

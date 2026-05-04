pub(crate) const VERSION_TOKEN_HEX_LEN: usize = 16;

pub(crate) fn version_token_for_bytes(bytes: &[u8]) -> String {
    let full_hash = blake3::hash(bytes);
    let hex = full_hash.to_hex();
    hex.as_str()[..VERSION_TOKEN_HEX_LEN].to_string()
}

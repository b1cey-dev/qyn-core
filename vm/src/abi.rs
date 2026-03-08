//! ABI encoding/decoding for contract calls (Ethereum-compatible).
//! Use alloy_sol_types::SolCall for encode; decode via SolInterface or manual parsing.


/// Encode contract call selector + args (first 4 bytes = selector, rest = abi-encoded params).
pub fn encode_selector_and_args(selector: [u8; 4], args: &[u8]) -> Vec<u8> {
    let mut out = selector.to_vec();
    out.extend_from_slice(args);
    out
}

/// Decode revert reason from revert payload if present.
pub fn decode_revert_reason(data: &[u8]) -> Option<String> {
    if data.len() < 4 {
        return None;
    }
    // Error(string) selector 0x08c379a0
    if data[0..4] == [0x08, 0xc3, 0x79, 0xa0] && data.len() > 68 {
        let len = u32::from_be_bytes(data[36..40].try_into().ok()?) as usize;
        if data.len() >= 40 + len {
            return std::str::from_utf8(&data[40..40 + len]).ok().map(String::from);
        }
    }
    None
}

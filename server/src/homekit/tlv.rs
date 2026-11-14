//! TLV8, the format of the pairing messages: one byte of type, one of length (at most 255), the
//! value; a longer value is split in fragments of the same type.

pub const METHOD: u8 = 0x00;
pub const IDENTIFIER: u8 = 0x01;
pub const SALT: u8 = 0x02;
pub const PUBLIC_KEY: u8 = 0x03;
pub const PROOF: u8 = 0x04;
pub const ENCRYPTED_DATA: u8 = 0x05;
pub const STATE: u8 = 0x06;
pub const ERROR: u8 = 0x07;
pub const SIGNATURE: u8 = 0x0a;

pub fn encode(items: &[(u8, &[u8])]) -> Vec<u8> {
    let mut out = vec![];
    for (kind, value) in items {
        if value.is_empty() {
            out.extend_from_slice(&[*kind, 0]);
            continue;
        }
        for chunk in value.chunks(255) {
            out.push(*kind);
            out.push(chunk.len() as u8);
            out.extend_from_slice(chunk);
        }
    }
    out
}

/// The items of a message, a value split in fragments put back together.
pub fn decode(buf: &[u8]) -> Result<Vec<(u8, Vec<u8>)>, String> {
    let mut out: Vec<(u8, Vec<u8>)> = vec![];
    let mut i = 0;
    let mut last_len = 0;
    while i < buf.len() {
        let (kind, len) = (buf[i], *buf.get(i + 1).ok_or("truncated TLV")? as usize);
        let value = buf.get(i + 2..i + 2 + len).ok_or("truncated TLV")?;
        match out.last_mut() {
            Some((k, v)) if *k == kind && last_len == 255 => v.extend_from_slice(value),
            _ => out.push((kind, value.to_vec())),
        }
        last_len = len;
        i += 2 + len;
    }
    Ok(out)
}

pub fn get(items: &[(u8, Vec<u8>)], kind: u8) -> Option<&[u8]> {
    items.iter().find(|(k, _)| *k == kind).map(|(_, v)| v.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_values_are_split_and_joined() {
        let long = vec![7u8; 600];
        let buf = encode(&[(STATE, &[1]), (PUBLIC_KEY, &long), (SALT, &[])]);
        assert_eq!(buf.len(), 3 + (2 * 3 + 600) + 2);
        let items = decode(&buf).unwrap();
        assert_eq!(get(&items, STATE), Some(&[1u8][..]));
        assert_eq!(get(&items, PUBLIC_KEY).unwrap().len(), 600);
        assert_eq!(get(&items, SALT), Some(&[][..]));
        assert!(decode(&[STATE, 3, 1]).is_err());
    }
}

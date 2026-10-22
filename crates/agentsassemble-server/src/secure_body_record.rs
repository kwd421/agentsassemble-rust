//! Authenticated HTTP body plaintext; outer AEAD still owns sequence/order.
use crate::secure_queue::CHUNK_BYTES;

const HEADER: usize = 18;
pub(crate) const REQUEST: u8 = 1;
pub(crate) const RESPONSE: u8 = 2;
pub(crate) const CAPABILITY: &str = "x-agentsassemble-body-binary";
pub(crate) const CONFIRMED: &str = "x-agentsassemble-body-binary-confirmed";

pub(crate) struct Record<'a> {
    pub(crate) id: u32,
    pub(crate) offset: u64,
    pub(crate) bytes: &'a [u8],
    pub(crate) end: bool,
}

pub(crate) fn decode(plaintext: &[u8], direction: u8) -> Result<Record<'_>, ()> {
    if plaintext.len() < HEADER || plaintext[0] != direction || plaintext[1] > 1 {
        return Err(());
    }
    let id = u32::from_be_bytes(plaintext[2..6].try_into().map_err(|_| ())?);
    let offset = u64::from_be_bytes(plaintext[6..14].try_into().map_err(|_| ())?);
    let length = u32::from_be_bytes(plaintext[14..18].try_into().map_err(|_| ())?) as usize;
    let end = plaintext[1] == 1;
    if id == 0
        || length > CHUNK_BYTES
        || plaintext.len() != HEADER + length
        || (end && length != 0)
        || (!end && length == 0)
        || offset.checked_add(length as u64).is_none()
    {
        return Err(());
    }
    Ok(Record {
        id,
        offset,
        bytes: &plaintext[HEADER..],
        end,
    })
}

pub(crate) fn encode(id: u32, offset: u64, bytes: &[u8], end: bool) -> Vec<u8> {
    let mut plaintext = Vec::with_capacity(HEADER + bytes.len());
    plaintext.extend_from_slice(&[RESPONSE, u8::from(end)]);
    plaintext.extend_from_slice(&id.to_be_bytes());
    plaintext.extend_from_slice(&offset.to_be_bytes());
    // A record carries at most one chunk, far below u32::MAX.
    plaintext.extend_from_slice(&u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_be_bytes());
    plaintext.extend_from_slice(bytes);
    plaintext
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_body_round_trip_and_malformed_boundary() {
        let bytes = (0..CHUNK_BYTES)
            .map(|i| u8::try_from(i % 256).unwrap())
            .collect::<Vec<_>>();
        let plaintext = encode(7, 123, &bytes, false);
        let record = decode(&plaintext, RESPONSE).expect("body");
        assert_eq!((record.id, record.offset, record.end), (7, 123, false));
        assert_eq!(record.bytes, bytes);
        assert!(decode(&plaintext, REQUEST).is_err());
        assert!(decode(&plaintext[..plaintext.len() - 1], RESPONSE).is_err());
        let mut malformed = plaintext.clone();
        malformed[1] = 2;
        assert!(decode(&malformed, RESPONSE).is_err());
        malformed[1] = 1;
        assert!(decode(&malformed, RESPONSE).is_err());
        assert!(decode(&encode(0, 0, &[], true), RESPONSE).is_err());
        assert!(decode(&encode(1, u64::MAX, &[1], false), RESPONSE).is_err());
        assert!(decode(&encode(1, 0, &vec![0; CHUNK_BYTES + 1], false), RESPONSE).is_err());
        assert!(decode(&encode(1, 0, &[], false), RESPONSE).is_err());
        assert!(
            decode(&encode(1, 123, &[], true), RESPONSE)
                .expect("end")
                .end
        );
    }
}

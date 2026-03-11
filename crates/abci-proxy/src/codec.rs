use bytes::{Buf, BytesMut};
use prost::encoding::{decode_varint, encode_varint};
use tokio_util::codec::{Decoder, Encoder};

pub struct VarintCodec;

impl Decoder for VarintCodec {
    type Item = BytesMut;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let mut probe = &src[..];
        let msg_len = match decode_varint(&mut probe) {
            Ok(len) => len as usize,
            Err(_) => return Ok(None),
        };

        let header_len = src.len() - probe.remaining();
        let total = header_len + msg_len;
        if src.len() < total {
            src.reserve(total - src.len());
            return Ok(None);
        }

        src.advance(header_len);
        Ok(Some(src.split_to(msg_len)))
    }
}

impl Encoder<bytes::Bytes> for VarintCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: bytes::Bytes, dst: &mut BytesMut) -> Result<(), Self::Error> {
        encode_varint(item.len() as u64, dst);
        dst.extend_from_slice(&item);
        Ok(())
    }
}

use tokio_util::codec::{Decoder, Encoder};
use tokio_util::bytes::{BytesMut, Buf};
use serde_json::Value;
use std::io;

pub struct KlippyUdsCodec;

impl Decoder for KlippyUdsCodec {
    type Item = Value;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(n) = src.iter().position(|&b| b == b'\n') {
            let line = src.split_to(n);
            src.advance(1); // Skip the newline character itself
            if line.is_empty() {
                return Ok(None);
            }
            let val: Value = serde_json::from_slice(&line)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            return Ok(Some(val));
        }
        Ok(None)
    }
}

impl Encoder<Value> for KlippyUdsCodec {
    type Error = io::Error;

    fn encode(&mut self, item: Value, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let bytes = serde_json::to_vec(&item)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        dst.extend_from_slice(&bytes);
        dst.extend_from_slice(b"\n");
        Ok(())
    }
}

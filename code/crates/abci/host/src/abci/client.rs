use std::path::Path;

use futures_util::{SinkExt, StreamExt};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;
use tokio_util::codec::{FramedRead, FramedWrite};

use tendermint_proto::v0_38::abci;

pub struct AbciClient {
    read: FramedRead<OwnedReadHalf, Decode<abci::Response>>,
    write: FramedWrite<OwnedWriteHalf, Encode<abci::Request>>,
}

impl AbciClient {
    pub async fn connect(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let stream = UnixStream::connect(path).await?;
        let (read, write) = stream.into_split();

        Ok(Self {
            read: FramedRead::new(read, Decode::default()),
            write: FramedWrite::new(write, Encode::default()),
        })
    }

    pub async fn request(&mut self, request: abci::Request) -> Result<abci::Response, eyre::Error> {
        self.write.send(request).await?;
        self.read.next().await.ok_or(eyre::eyre!("no response"))?
    }

    // The ABCI server expects flush to be called after every synchronous request.
    // If the function above is used, the value won't be returned until Flush
    // is called at a later time
    pub async fn request_with_flush(
        &mut self,
        request: abci::Request,
    ) -> Result<abci::Response, eyre::Error> {
        self.write.send(request).await?;

        let request = abci::Request {
            value: Some(abci::request::Value::Flush(abci::RequestFlush {})),
        };

        // Send request
        self.write.send(request).await?;

        // Read response
        let response = self.read.next().await.ok_or(eyre::eyre!("no response"))?;

        // Read and discard Flush response
        self.read.next().await;

        response
    }
}

use std::marker::PhantomData;

use tokio_util::codec::{Decoder, Encoder};

use bytes::{BufMut, BytesMut};

pub struct Decode<M> {
    state: DecodeState,
    _marker: PhantomData<M>,
}

impl<M> Default for Decode<M> {
    fn default() -> Self {
        Self {
            state: DecodeState::Head,
            _marker: PhantomData,
        }
    }
}

#[derive(Debug)]
enum DecodeState {
    Head,
    Body { len: usize },
}

impl<M: prost::Message + Default> Decoder for Decode<M> {
    type Item = M;
    type Error = eyre::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        match self.state {
            DecodeState::Head => {
                tracing::trace!(?src, "decoding head");
                // we don't use decode_varint directly, because it advances the
                // buffer regardless of success, but Decoder assumes that when
                // the buffer advances we've consumed the data. this is sort of
                // a sad hack, but it works.
                // TODO(erwan): fix this

                // Tendermint socket protocol:
                //   "Messages are serialized using Protobuf3 and length-prefixed
                //    with an unsigned varint"
                // See: https://github.com/tendermint/tendermint/blob/v0.38.x/spec/abci/abci++_client_server.md#socket
                let mut tmp = src.clone().freeze();
                let len = match prost::encoding::decode_varint(&mut tmp) {
                    Ok(_) => {
                        // advance the real buffer
                        prost::encoding::decode_varint(src).unwrap() as usize
                    }
                    Err(_) => {
                        tracing::trace!(?self.state, src.len = src.len(), "waiting for header data");
                        return Ok(None);
                    }
                };
                self.state = DecodeState::Body { len };
                tracing::trace!(?self.state, "ready for body");

                // Recurse to attempt body decoding.
                self.decode(src)
            }
            DecodeState::Body { len } => {
                if src.len() < len {
                    tracing::trace!(?self.state, src.len = src.len(), "waiting for body");
                    return Ok(None);
                }

                let body = src.split_to(len);
                tracing::trace!(?body, "decoding body");
                let message = M::decode(body)?;

                // Now reset the decoder state for the next message.
                self.state = DecodeState::Head;

                Ok(Some(message))
            }
        }
    }
}

pub struct Encode<M> {
    _marker: PhantomData<M>,
}

impl<M> Default for Encode<M> {
    fn default() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<M: prost::Message + Sized + std::fmt::Debug> Encoder<M> for Encode<M> {
    type Error = eyre::Error;

    fn encode(&mut self, item: M, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let mut buf = BytesMut::new();
        item.encode(&mut buf)?;
        let buf = buf.freeze();

        // Tendermint socket protocol:
        //   "Messages are serialized using Protobuf3 and length-prefixed
        //    with an unsigned varint"
        // See: https://github.com/tendermint/tendermint/blob/v0.38.x/spec/abci/abci++_client_server.md#socket
        prost::encoding::encode_varint(buf.len() as u64, dst);
        dst.put(buf);

        Ok(())
    }
}

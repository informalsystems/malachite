use malachite_starknet_p2p_types::Block;
use prost::Message;

use malachite_actors::util::codec::NetworkCodec;
use malachite_actors::util::streaming::{StreamContent, StreamMessage};
use malachite_blocksync::{
    self as blocksync, BlockRequest, BlockResponse, VoteSetRequest, VoteSetResponse,
};
use malachite_common::{
    AggregatedSignature, CommitCertificate, CommitSignature, Extension, Round, SignedExtension,
    SignedProposal, SignedVote,
};
use malachite_consensus::SignedConsensusMsg;
use malachite_gossip_consensus::Bytes;

use crate::proto::consensus_message::Messages;
use crate::proto::{self as proto, ConsensusMessage, Error as ProtoError, Protobuf};
use crate::types::MockContext;
use crate::types::{self as p2p, Address, BlockHash, Height, ProposalPart, Vote};

pub struct ProtobufCodec;

impl NetworkCodec<ProposalPart> for ProtobufCodec {
    type Error = ProtoError;

    fn decode(&self, bytes: Bytes) -> Result<ProposalPart, Self::Error> {
        ProposalPart::from_bytes(bytes.as_ref())
    }

    fn encode(&self, msg: ProposalPart) -> Result<Bytes, Self::Error> {
        msg.to_bytes()
    }
}

impl NetworkCodec<blocksync::Status<MockContext>> for ProtobufCodec {
    type Error = ProtoError;

    fn decode(&self, bytes: Bytes) -> Result<blocksync::Status<MockContext>, Self::Error> {
        let status = proto::sync::Status::decode(bytes.as_ref()).map_err(ProtoError::Decode)?;

        let peer_id = status
            .peer_id
            .ok_or_else(|| ProtoError::missing_field::<proto::sync::Status>("peer_id"))?;

        Ok(blocksync::Status {
            peer_id: libp2p_identity::PeerId::from_bytes(&peer_id.id)
                .map_err(|e| ProtoError::Other(e.to_string()))?,
            height: Height::new(status.block_number, status.fork_id),
            earliest_block_height: Height::new(
                status.earliest_block_number,
                status.earliest_fork_id,
            ),
        })
    }

    fn encode(&self, status: blocksync::Status<MockContext>) -> Result<Bytes, Self::Error> {
        let proto = proto::sync::Status {
            peer_id: Some(proto::PeerId {
                id: Bytes::from(status.peer_id.to_bytes()),
            }),
            block_number: status.height.block_number,
            fork_id: status.height.fork_id,
            earliest_block_number: status.earliest_block_height.block_number,
            earliest_fork_id: status.earliest_block_height.fork_id,
        };

        Ok(Bytes::from(proto.encode_to_vec()))
    }
}

impl NetworkCodec<blocksync::Request<MockContext>> for ProtobufCodec {
    type Error = ProtoError;

    fn decode(&self, bytes: Bytes) -> Result<blocksync::Request<MockContext>, Self::Error> {
        let proto_request = proto::sync::SyncRequest::decode(bytes)
            .map_err(ProtoError::Decode)?
            .messages
            .ok_or_else(|| ProtoError::missing_field::<proto::sync::SyncRequest>("messages"))?;

        let request = match proto_request {
            proto::sync::sync_request::Messages::BlockRequest(block_request) => {
                blocksync::Request::BlockRequest(BlockRequest::new(Height::new(
                    block_request.block_number,
                    block_request.fork_id,
                )))
            }
            proto::sync::sync_request::Messages::VoteSetRequest(vote_set_request) => {
                blocksync::Request::VoteSetRequest(VoteSetRequest::new(
                    Height::new(vote_set_request.block_number, vote_set_request.fork_id),
                    Round::new(vote_set_request.round),
                ))
            }
        };

        Ok(request)
    }

    fn encode(&self, request: blocksync::Request<MockContext>) -> Result<Bytes, Self::Error> {
        let proto = match request {
            blocksync::Request::BlockRequest(block_request) => proto::sync::SyncRequest {
                messages: Some(proto::sync::sync_request::Messages::BlockRequest(
                    proto::sync::BlockRequest {
                        fork_id: block_request.height.fork_id,
                        block_number: block_request.height.block_number,
                    },
                )),
            },
            blocksync::Request::VoteSetRequest(vote_set_request) => proto::sync::SyncRequest {
                messages: Some(proto::sync::sync_request::Messages::VoteSetRequest(
                    proto::sync::VoteSetRequest {
                        fork_id: vote_set_request.height.fork_id,
                        block_number: vote_set_request.height.block_number,
                        round: vote_set_request
                            .round
                            .as_u32()
                            .expect("round should not be nil"),
                    },
                )),
            },
        };

        Ok(Bytes::from(proto.encode_to_vec()))
    }
}

impl NetworkCodec<blocksync::Response<MockContext>> for ProtobufCodec {
    type Error = ProtoError;

    fn decode(&self, bytes: Bytes) -> Result<blocksync::Response<MockContext>, Self::Error> {
        let proto_request = proto::sync::SyncResponse::decode(bytes)
            .map_err(ProtoError::Decode)?
            .messages
            .ok_or_else(|| ProtoError::missing_field::<proto::sync::SyncResponse>("messages"))?;

        let response = match proto_request {
            proto::sync::sync_response::Messages::BlockResponse(block_response) => {
                blocksync::Response::BlockResponse(BlockResponse::new(
                    Height::new(block_response.block_number, block_response.fork_id),
                    block_response.block.map(decode_synced_block).transpose()?,
                ))
            }
            proto::sync::sync_response::Messages::VoteSetResponse(vote_set_response) => {
                let vote_set = vote_set_response
                    .vote_set
                    .ok_or_else(|| ProtoError::missing_field::<proto::sync::VoteSet>("vote_set"))?;

                blocksync::Response::VoteSetResponse(VoteSetResponse::new(decode_vote_set(
                    vote_set,
                )?))
            }
        };
        Ok(response)
    }

    fn encode(&self, response: blocksync::Response<MockContext>) -> Result<Bytes, Self::Error> {
        let proto = match response {
            blocksync::Response::BlockResponse(block_response) => proto::sync::SyncResponse {
                messages: Some(proto::sync::sync_response::Messages::BlockResponse(
                    proto::sync::BlockResponse {
                        fork_id: block_response.height.fork_id,
                        block_number: block_response.height.block_number,
                        block: block_response.block.map(encode_synced_block).transpose()?,
                    },
                )),
            },
            blocksync::Response::VoteSetResponse(vote_set_response) => proto::sync::SyncResponse {
                messages: Some(proto::sync::sync_response::Messages::VoteSetResponse(
                    proto::sync::VoteSetResponse {
                        vote_set: Some(encode_vote_set(vote_set_response.vote_set)?),
                    },
                )),
            },
        };

        Ok(Bytes::from(proto.encode_to_vec()))
    }
}

impl NetworkCodec<SignedConsensusMsg<MockContext>> for ProtobufCodec {
    type Error = ProtoError;

    fn decode(&self, bytes: Bytes) -> Result<SignedConsensusMsg<MockContext>, Self::Error> {
        let proto = proto::ConsensusMessage::decode(bytes)?;

        let proto_signature = proto
            .signature
            .ok_or_else(|| ProtoError::missing_field::<proto::ConsensusMessage>("signature"))?;

        let message = proto
            .messages
            .ok_or_else(|| ProtoError::missing_field::<proto::ConsensusMessage>("messages"))?;

        let signature = p2p::Signature::from_proto(proto_signature)?;

        match message {
            Messages::Vote(v) => {
                Vote::from_proto(v).map(|v| SignedConsensusMsg::Vote(SignedVote::new(v, signature)))
            }
            Messages::Proposal(p) => p2p::Proposal::from_proto(p)
                .map(|p| SignedConsensusMsg::Proposal(SignedProposal::new(p, signature))),
        }
    }

    fn encode(&self, msg: SignedConsensusMsg<MockContext>) -> Result<Bytes, Self::Error> {
        let message = match msg {
            SignedConsensusMsg::Vote(v) => proto::ConsensusMessage {
                messages: Some(Messages::Vote(v.to_proto()?)),
                signature: Some(v.signature.to_proto()?),
            },
            SignedConsensusMsg::Proposal(p) => proto::ConsensusMessage {
                messages: Some(Messages::Proposal(p.to_proto()?)),
                signature: Some(p.signature.to_proto()?),
            },
        };

        Ok(Bytes::from(prost::Message::encode_to_vec(&message)))
    }
}

impl<T> NetworkCodec<StreamMessage<T>> for ProtobufCodec
where
    T: Protobuf,
{
    type Error = ProtoError;

    fn decode(&self, bytes: Bytes) -> Result<StreamMessage<T>, Self::Error> {
        let p2p_msg = p2p::StreamMessage::from_bytes(&bytes)?;
        Ok(StreamMessage {
            stream_id: p2p_msg.id,
            sequence: p2p_msg.sequence,
            content: match p2p_msg.content {
                p2p::StreamContent::Data(data) => {
                    StreamContent::Data(T::from_bytes(data.as_ref())?)
                }
                p2p::StreamContent::Fin(fin) => StreamContent::Fin(fin),
            },
        })
    }

    fn encode(&self, msg: StreamMessage<T>) -> Result<Bytes, Self::Error> {
        let p2p_msg = p2p::StreamMessage {
            id: msg.stream_id,
            sequence: msg.sequence,
            content: match msg.content {
                StreamContent::Data(data) => p2p::StreamContent::Data(data.to_bytes()?),
                StreamContent::Fin(fin) => p2p::StreamContent::Fin(fin),
            },
        };

        p2p_msg.to_bytes()
    }
}

pub(crate) fn encode_synced_block(
    synced_block: blocksync::SyncedBlock<MockContext>,
) -> Result<proto::sync::SyncedBlock, ProtoError> {
    Ok(proto::sync::SyncedBlock {
        block_bytes: synced_block.block_bytes,
        certificate: Some(encode_certificate(synced_block.certificate)?),
    })
}

pub(crate) fn decode_synced_block(
    proto: proto::sync::SyncedBlock,
) -> Result<blocksync::SyncedBlock<MockContext>, ProtoError> {
    let Some(certificate) = proto.certificate else {
        return Err(ProtoError::missing_field::<proto::sync::SyncedBlock>(
            "certificate",
        ));
    };

    Ok(blocksync::SyncedBlock {
        block_bytes: proto.block_bytes,
        certificate: decode_certificate(certificate)?,
    })
}

pub(crate) fn encode_aggregate_signature(
    aggregated_signature: AggregatedSignature<MockContext>,
) -> Result<proto::sync::AggregatedSignature, ProtoError> {
    let signatures = aggregated_signature
        .signatures
        .into_iter()
        .map(|s| {
            let validator_address = s.address.to_proto()?;
            let signature = s.signature.to_proto()?;

            Ok(proto::sync::CommitSignature {
                validator_address: Some(validator_address),
                signature: Some(signature),
                extension: s.extension.map(encode_extension).transpose()?,
            })
        })
        .collect::<Result<_, ProtoError>>()?;

    Ok(proto::sync::AggregatedSignature { signatures })
}

pub(crate) fn decode_aggregated_signature(
    signature: proto::sync::AggregatedSignature,
) -> Result<AggregatedSignature<MockContext>, ProtoError> {
    let signatures = signature
        .signatures
        .into_iter()
        .map(|s| {
            let signature = s
                .signature
                .ok_or_else(|| {
                    ProtoError::missing_field::<proto::sync::CommitSignature>("signature")
                })
                .and_then(p2p::Signature::from_proto)?;

            let address = s
                .validator_address
                .ok_or_else(|| {
                    ProtoError::missing_field::<proto::sync::CommitSignature>("validator_address")
                })
                .and_then(Address::from_proto)?;

            let extension = s.extension.map(decode_extension).transpose()?;

            Ok(CommitSignature {
                address,
                signature,
                extension,
            })
        })
        .collect::<Result<Vec<_>, ProtoError>>()?;

    Ok(AggregatedSignature { signatures })
}

pub(crate) fn encode_extension(
    ext: SignedExtension<MockContext>,
) -> Result<proto::Extension, ProtoError> {
    Ok(proto::Extension {
        data: ext.message.data,
        signature: Some(ext.signature.to_proto()?),
    })
}

pub(crate) fn decode_extension(
    ext: proto::Extension,
) -> Result<SignedExtension<MockContext>, ProtoError> {
    let extension = Extension::from(ext.data);
    let signature = ext
        .signature
        .ok_or_else(|| ProtoError::missing_field::<proto::Extension>("signature"))
        .and_then(p2p::Signature::from_proto)?;

    Ok(SignedExtension::new(extension, signature))
}

pub(crate) fn encode_certificate(
    certificate: CommitCertificate<MockContext>,
) -> Result<proto::sync::CommitCertificate, ProtoError> {
    Ok(proto::sync::CommitCertificate {
        fork_id: certificate.height.fork_id,
        block_number: certificate.height.block_number,
        round: certificate.round.as_u32().expect("round should not be nil"),
        block_hash: Some(certificate.value_id.to_proto()?),
        aggregated_signature: Some(encode_aggregate_signature(
            certificate.aggregated_signature,
        )?),
    })
}

pub(crate) fn decode_certificate(
    certificate: proto::sync::CommitCertificate,
) -> Result<CommitCertificate<MockContext>, ProtoError> {
    let value_id = if let Some(block_hash) = certificate.block_hash {
        BlockHash::from_proto(block_hash)?
    } else {
        return Err(ProtoError::missing_field::<proto::sync::CommitCertificate>(
            "block_hash",
        ));
    };

    let aggregated_signature = if let Some(agg_sig) = certificate.aggregated_signature {
        decode_aggregated_signature(agg_sig)?
    } else {
        return Err(ProtoError::missing_field::<proto::sync::CommitCertificate>(
            "aggregated_signature",
        ));
    };

    let certificate = CommitCertificate {
        height: Height::new(certificate.block_number, certificate.fork_id),
        round: Round::new(certificate.round),
        value_id,
        aggregated_signature,
    };

    Ok(certificate)
}

pub(crate) fn encode_block(block: &Block) -> Result<Vec<u8>, ProtoError> {
    let proto = proto::sync::Block {
        fork_id: block.height.fork_id,
        block_number: block.height.block_number,
        transactions: Some(block.transactions.to_proto()?),
        block_hash: Some(block.block_hash.to_proto()?),
    };

    Ok(proto.encode_to_vec())
}

pub(crate) fn encode_vote_set(
    vote_set: malachite_common::VoteSet<MockContext>,
) -> Result<proto::sync::VoteSet, ProtoError> {
    Ok(proto::sync::VoteSet {
        signed_votes: vote_set
            .vote_set
            .into_iter()
            .map(encode_vote)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

pub(crate) fn encode_vote(vote: SignedVote<MockContext>) -> Result<ConsensusMessage, ProtoError> {
    Ok(ConsensusMessage {
        messages: Some(Messages::Vote(vote.message.to_proto()?)),
        signature: Some(vote.signature.to_proto()?),
    })
}

pub(crate) fn decode_vote_set(
    vote_set: proto::sync::VoteSet,
) -> Result<malachite_common::VoteSet<MockContext>, ProtoError> {
    Ok(malachite_common::VoteSet {
        vote_set: vote_set
            .signed_votes
            .into_iter()
            .filter_map(decode_vote)
            .collect(),
    })
}

pub(crate) fn decode_vote(msg: ConsensusMessage) -> Option<SignedVote<MockContext>> {
    let signature = msg.signature?;
    let vote = match msg.messages {
        Some(Messages::Vote(v)) => Some(v),
        _ => None,
    }?;

    let signature = p2p::Signature::from_proto(signature).ok()?;
    let vote = Vote::from_proto(vote).ok()?;
    Some(SignedVote::new(vote, signature))
}

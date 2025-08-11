use bytes::Bytes;
use malachitebft_signing_ed25519::Signature;
use serde::{Deserialize, Serialize};

use malachitebft_core_types::Round;
use malachitebft_proto::{Error as ProtoError, Protobuf};

use crate::types::{
    address::Address, context::MockContext, hash::Hash, height::Height, transaction::Transaction,
};

use super::proto;
use crate::codec::{decode_signature, encode_signature};
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalData {
    pub transactions: Vec<Transaction>,
}

impl ProposalData {
    pub fn new(transactions: Vec<Transaction>) -> Self {
        Self { transactions }
    }

    pub fn size_bytes(&self) -> usize {
        std::mem::size_of::<u64>()
    }
}

impl std::fmt::Debug for ProposalData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ProposalData {{ {} transactions }}",
            self.transactions.len()
        )
    }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "Round")]
enum RoundDef {
    Nil,
    Some(u32),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PartType {
    Init,
    Data,
    Fin,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalPart {
    Init(ProposalInit),
    Data(ProposalData),
    Fin(ProposalFin),
}

impl ProposalPart {
    pub fn part_type(&self) -> PartType {
        match self {
            Self::Init(_) => PartType::Init,
            Self::Data(_) => PartType::Data,
            Self::Fin(_) => PartType::Fin,
        }
    }

    pub fn get_type(&self) -> &'static str {
        match self {
            Self::Init(_) => "init",
            Self::Data(_) => "data",
            Self::Fin(_) => "fin",
        }
    }

    pub fn as_init(&self) -> Option<&ProposalInit> {
        match self {
            Self::Init(init) => Some(init),
            _ => None,
        }
    }

    pub fn as_data(&self) -> Option<&ProposalData> {
        match self {
            Self::Data(data) => Some(data),
            _ => None,
        }
    }

    pub fn as_fin(&self) -> Option<&ProposalFin> {
        match self {
            Self::Fin(fin) => Some(fin),
            _ => None,
        }
    }

    pub fn tx_count(&self) -> usize {
        match self {
            Self::Data(data) => data.transactions.len(),
            _ => 0,
        }
    }

    pub fn to_sign_bytes(&self) -> Bytes {
        Protobuf::to_bytes(self).unwrap()
    }

    pub fn size_bytes(&self) -> usize {
        self.to_sign_bytes().len() // TODO: Do this more efficiently
    }
}

/// A part of a value for a height, round. Identified in this scope by the sequence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalInit {
    pub height: Height,
    #[serde(with = "RoundDef")]
    pub round: Round,
    #[serde(with = "RoundDef")]
    pub valid_round: Round,
    pub proposer: Address,
}

impl ProposalInit {
    pub fn new(height: Height, round: Round, valid_round: Round, proposer: Address) -> Self {
        Self {
            height,
            round,
            valid_round,
            proposer,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalFin {
    pub signature: Signature,
    pub commitment: Hash,
}

impl ProposalFin {
    pub fn new(signature: Signature, commitment: Hash) -> Self {
        Self {
            signature,
            commitment,
        }
    }
}

impl malachitebft_core_types::ProposalPart<MockContext> for ProposalPart {
    fn is_first(&self) -> bool {
        matches!(self, Self::Init(_))
    }

    fn is_last(&self) -> bool {
        matches!(self, Self::Fin(_))
    }
}

impl Protobuf for ProposalPart {
    type Proto = proto::ProposalPart;

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn from_proto(proto: Self::Proto) -> Result<Self, ProtoError> {
        use proto::proposal_part::Part;

        let part = proto
            .part
            .ok_or_else(|| ProtoError::missing_field::<Self::Proto>("part"))?;

        match part {
            Part::Init(init) => Ok(Self::Init(ProposalInit {
                height: Height::new(init.height),
                round: Round::new(init.round),
                valid_round: Round::from(init.pol_round),
                proposer: init
                    .proposer
                    .ok_or_else(|| ProtoError::missing_field::<Self::Proto>("proposer"))
                    .and_then(Address::from_proto)?,
            })),
            Part::Data(data) => Ok(Self::Data(ProposalData {
                transactions: data
                    .transactions
                    .map(|batch| {
                        batch
                            .transactions
                            .into_iter()
                            .map(Transaction::from_proto)
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?
                    .unwrap_or_default(),
            })),
            Part::Fin(fin) => {
                let proto_hash: proto::Hash = fin
                    .commitment
                    .ok_or_else(|| ProtoError::missing_field::<Self::Proto>("commitment"))?;

                let commitment: Hash = Hash::from_bytes(&proto_hash.elements)
                    .map_err(|_| ProtoError::invalid_data::<proto::ProposalPart>("invalid hash"))?;

                Ok(Self::Fin(ProposalFin {
                    signature: fin
                        .signature
                        .ok_or_else(|| ProtoError::missing_field::<Self::Proto>("signature"))
                        .and_then(decode_signature)?,
                    commitment,
                }))
            }
        }
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn to_proto(&self) -> Result<Self::Proto, ProtoError> {
        use proto::proposal_part::Part;

        match self {
            Self::Init(init) => Ok(Self::Proto {
                part: Some(Part::Init(proto::ProposalInit {
                    height: init.height.as_u64(),
                    round: init.round.as_u32().unwrap(),
                    pol_round: init.valid_round.as_u32(),
                    proposer: Some(init.proposer.to_proto()?),
                })),
            }),
            Self::Data(data) => Ok(Self::Proto {
                part: Some(Part::Data(proto::ProposalData {
                    transactions: Some(proto::TransactionBatch {
                        transactions: data
                            .transactions
                            .iter()
                            .map(|t| t.to_proto())
                            .collect::<Result<Vec<_>, _>>()?,
                    }),
                })),
            }),
            Self::Fin(fin) => Ok(Self::Proto {
                part: Some(Part::Fin(proto::ProposalFin {
                    signature: Some(encode_signature(&fin.signature)),
                    commitment: Some(proto::Hash {
                        elements: Bytes::from(fin.commitment.to_vec()),
                    }),
                })),
            }),
        }
    }
}

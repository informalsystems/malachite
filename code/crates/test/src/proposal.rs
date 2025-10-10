use bytes::Bytes;
use malachitebft_core_types::Round;
use malachitebft_proto::{Error as ProtoError, Protobuf};
// FaB: Import Certificate for proposal justification (4f+1 prevotes)
use malachitebft_core_state_machine::input::Certificate;

use crate::{Address, Height, TestContext, Value};

/// A proposal for a value in a round
/// FaB: Now includes optional certificate for rounds > 0
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    pub height: Height,
    pub round: Round,
    pub value: Value,
    pub pol_round: Round,
    pub validator_address: Address,
    /// FaB: Certificate containing 4f+1 prevotes to justify this proposal
    /// Required for rounds > 0, None for round 0
    pub certificate: Option<Certificate<TestContext>>,
}

impl Proposal {
    pub fn new(
        height: Height,
        round: Round,
        value: Value,
        pol_round: Round,
        validator_address: Address,
        certificate: Option<Certificate<TestContext>>,
    ) -> Self {
        Self {
            height,
            round,
            value,
            pol_round,
            validator_address,
            certificate,
        }
    }

    pub fn to_sign_bytes(&self) -> Bytes {
        // FaB: Sign proposal WITHOUT certificate
        // The certificate is evidence (already signed by voters), not commitment by proposer
        // Signature covers: height, round, value, pol_round, validator_address only
        let proto = crate::proto::Proposal {
            height: self.height.to_proto().unwrap(),
            round: self.round.as_u32().expect("round should not be nil"),
            value: Some(self.value.to_proto().unwrap()),
            pol_round: self.pol_round.as_u32(),
            validator_address: Some(self.validator_address.to_proto().unwrap()),
            certificate: None,  // ← ALWAYS None for signing (transmitted separately)
        };

        use prost::Message;
        Bytes::from(proto.encode_to_vec())
    }

    pub fn from_sign_bytes(bytes: &[u8]) -> Result<Self, ProtoError> {
        Protobuf::from_bytes(bytes)
    }

    /// FaB: Create a new proposal with the certificate field populated
    /// Used in consensus layer to merge certificate before signing/broadcasting
    pub fn with_certificate(mut self, certificate: Option<Certificate<TestContext>>) -> Self {
        self.certificate = certificate;
        self
    }
}

impl malachitebft_core_types::Proposal<TestContext> for Proposal {
    fn height(&self) -> Height {
        self.height
    }

    fn round(&self) -> Round {
        self.round
    }

    fn value(&self) -> &Value {
        &self.value
    }

    fn take_value(self) -> Value {
        self.value
    }

    fn pol_round(&self) -> Round {
        self.pol_round
    }

    fn validator_address(&self) -> &Address {
        &self.validator_address
    }
}

impl Protobuf for Proposal {
    type Proto = crate::proto::Proposal;

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn to_proto(&self) -> Result<Self::Proto, ProtoError> {
        // FaB: Encode certificate if present (rounds > 0)
        let certificate = match &self.certificate {
            Some(cert) => Some(crate::codec::proto::encode_certificate(cert)?),
            None => None,
        };

        Ok(Self::Proto {
            height: self.height.to_proto()?,
            round: self.round.as_u32().expect("round should not be nil"),
            value: Some(self.value.to_proto()?),
            pol_round: self.pol_round.as_u32(),
            validator_address: Some(self.validator_address.to_proto()?),
            certificate,
        })
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn from_proto(proto: Self::Proto) -> Result<Self, ProtoError> {
        // FaB: Decode certificate if present (rounds > 0)
        let certificate = match proto.certificate {
            Some(cert) => Some(crate::codec::proto::decode_certificate(cert)?),
            None => None,
        };

        Ok(Self {
            height: Height::from_proto(proto.height)?,
            round: Round::new(proto.round),
            value: Value::from_proto(
                proto
                    .value
                    .ok_or_else(|| ProtoError::missing_field::<Self::Proto>("value"))?,
            )?,
            pol_round: Round::from(proto.pol_round),
            validator_address: Address::from_proto(
                proto
                    .validator_address
                    .ok_or_else(|| ProtoError::missing_field::<Self::Proto>("validator_address"))?,
            )?,
            certificate,
        })
    }
}

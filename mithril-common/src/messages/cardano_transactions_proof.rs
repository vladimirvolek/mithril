use crate::entities::{
    CardanoTransactionsSetProof, ProtocolMessage, ProtocolMessagePartKey, TransactionHash,
};
use crate::messages::CardanoTransactionsSetProofMessagePart;
use crate::StdError;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(target_family = "wasm")]
use wasm_bindgen::prelude::*;

/// A cryptographic proof for a set of Cardano transactions
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(
    target_family = "wasm",
    wasm_bindgen(getter_with_clone, js_name = "CardanoTransactionsProofs")
)]
pub struct CardanoTransactionsProofsMessage {
    /// Hash of the certificate that validate this proof merkle root
    pub certificate_hash: String,

    /// Transactions that have been certified
    pub certified_transactions: Vec<CardanoTransactionsSetProofMessagePart>,

    /// Transactions that could not be certified
    pub non_certified_transactions: Vec<TransactionHash>,

    /// Latest immutable file number associated to the Cardano Transactions
    pub latest_immutable_file_number: u64,
}

#[cfg_attr(
    target_family = "wasm",
    wasm_bindgen(js_class = "CardanoTransactionsProofs")
)]
impl CardanoTransactionsProofsMessage {
    /// Transactions that have been certified
    #[cfg_attr(target_family = "wasm", wasm_bindgen(getter))]
    pub fn transactions_hashes(&self) -> Vec<TransactionHash> {
        self.certified_transactions
            .iter()
            .flat_map(|ct| ct.transactions_hashes.clone())
            .collect::<Vec<_>>()
    }
}

/// Set of transactions verified by [CardanoTransactionsProofsMessage::verify].
///
/// Can be used to reconstruct part of a [ProtocolMessage] in order to check that
/// it is indeed signed by a certificate.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedCardanoTransactions {
    certificate_hash: String,
    merkle_root: String,
    certified_transactions: Vec<TransactionHash>,
    latest_immutable_file_number: u64,
}

impl VerifiedCardanoTransactions {
    /// Hash of the certificate that signs this struct Merkle root.
    pub fn certificate_hash(&self) -> &str {
        &self.certificate_hash
    }

    /// Hashes of the certified transactions
    pub fn certified_transactions(&self) -> &[TransactionHash] {
        &self.certified_transactions
    }

    /// Fill the given [ProtocolMessage] with the data associated with this
    /// verified transactions set.
    pub fn fill_protocol_message(&self, message: &mut ProtocolMessage) {
        message.set_message_part(
            ProtocolMessagePartKey::CardanoTransactionsMerkleRoot,
            self.merkle_root.clone(),
        );

        message.set_message_part(
            ProtocolMessagePartKey::LatestImmutableFileNumber,
            self.latest_immutable_file_number.to_string(),
        );
    }
}

/// Error encountered or produced by the [cardano transaction proof verification][CardanoTransactionsProofsMessage::verify].
#[derive(Error, Debug)]
pub enum VerifyCardanoTransactionsProofsError {
    /// The verification of an individual [CardanoTransactionsSetProofMessagePart] failed.
    #[error("Invalid set proof for transactions hashes: {transactions_hashes:?}")]
    InvalidSetProof {
        /// Hashes of the invalid transactions
        transactions_hashes: Vec<TransactionHash>,
        /// Error source
        source: StdError,
    },

    /// No certified transactions set proof to verify
    #[error("There's no certified transaction to verify")]
    NoCertifiedTransaction,

    /// Not all certified transactions set proof have the same merkle root.
    ///
    /// This is problematic because all the set proof should be generated from the same
    /// merkle tree which root is signed in the [certificate][crate::entities::Certificate].
    #[error("All certified transactions set proofs must share the same Merkle root")]
    NonMatchingMerkleRoot,

    /// An individual [CardanoTransactionsSetProofMessagePart] could not be converted to a
    /// [CardanoTransactionsProofsMessage] for verification.
    #[error("Malformed data or unknown Cardano Set Proof format")]
    MalformedData(#[source] StdError),
}

impl CardanoTransactionsProofsMessage {
    /// Create a new `CardanoTransactionsProofsMessage`
    pub fn new(
        certificate_hash: &str,
        certified_transactions: Vec<CardanoTransactionsSetProofMessagePart>,
        non_certified_transactions: Vec<TransactionHash>,
        latest_immutable_file_number: u64,
    ) -> Self {
        Self {
            certificate_hash: certificate_hash.to_string(),
            certified_transactions,
            non_certified_transactions,
            latest_immutable_file_number,
        }
    }

    /// Verify that all the certified transactions proofs are valid
    ///
    /// The following checks will be executed:
    ///
    /// 1 - Check that each Merkle proof is valid
    ///
    /// 2 - Check that all proofs share the same Merkle root
    ///
    /// 3 - Assert that there's at least one certified transaction
    ///
    /// If every check is okay, the hex encoded Merkle root of the proof will be returned.
    pub fn verify(
        &self,
    ) -> Result<VerifiedCardanoTransactions, VerifyCardanoTransactionsProofsError> {
        let mut merkle_root = None;

        for certified_transaction in &self.certified_transactions {
            let certified_transaction: CardanoTransactionsSetProof = certified_transaction
                .clone()
                .try_into()
                .map_err(VerifyCardanoTransactionsProofsError::MalformedData)?;
            certified_transaction.verify().map_err(|e| {
                VerifyCardanoTransactionsProofsError::InvalidSetProof {
                    transactions_hashes: certified_transaction.transactions_hashes().to_vec(),
                    source: e,
                }
            })?;

            let tx_merkle_root = Some(certified_transaction.merkle_root());

            if merkle_root.is_none() {
                merkle_root = tx_merkle_root;
            } else if merkle_root != tx_merkle_root {
                return Err(VerifyCardanoTransactionsProofsError::NonMatchingMerkleRoot);
            }
        }

        Ok(VerifiedCardanoTransactions {
            certificate_hash: self.certificate_hash.clone(),
            merkle_root: merkle_root
                .ok_or(VerifyCardanoTransactionsProofsError::NoCertifiedTransaction)?,
            certified_transactions: self
                .certified_transactions
                .iter()
                .flat_map(|c| c.transactions_hashes.clone())
                .collect(),
            latest_immutable_file_number: self.latest_immutable_file_number,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use slog::Logger;

    use super::*;
    use crate::{
        cardano_transaction_parser::DumbTransactionParser,
        crypto_helper::MKProof,
        entities::{Beacon, CardanoTransaction},
        signable_builder::{
            CardanoTransactionsSignableBuilder, MockTransactionStore, SignableBuilder,
        },
    };

    #[test]
    fn verify_malformed_proofs_fail() {
        let txs_proofs = CardanoTransactionsProofsMessage::new(
            "whatever",
            vec![CardanoTransactionsSetProofMessagePart {
                transactions_hashes: vec![],
                proof: "invalid".to_string(),
            }],
            vec![],
            99999,
        );

        let error = txs_proofs
            .verify()
            .expect_err("Malformed txs proofs should fail to verify itself");
        assert!(
            matches!(
                error,
                VerifyCardanoTransactionsProofsError::MalformedData(_)
            ),
            "Expected 'MalformedData' error but got '{:?}'",
            error
        );
    }

    #[test]
    fn verify_no_certified_transaction_fail() {
        let txs_proofs = CardanoTransactionsProofsMessage::new("whatever", vec![], vec![], 99999);

        let error = txs_proofs
            .verify()
            .expect_err("Proofs without certified transactions should fail to verify itself");
        assert!(
            matches!(
                error,
                VerifyCardanoTransactionsProofsError::NoCertifiedTransaction
            ),
            "Expected 'NoCertifiedTransactions' error but got '{:?}'",
            error
        );
    }

    #[test]
    fn verify_valid_proofs() {
        let set_proof = CardanoTransactionsSetProof::dummy();
        let expected = VerifiedCardanoTransactions {
            certificate_hash: "whatever".to_string(),
            merkle_root: set_proof.merkle_root(),
            certified_transactions: set_proof.transactions_hashes().to_vec(),
            latest_immutable_file_number: 99999,
        };
        let txs_proofs = CardanoTransactionsProofsMessage::new(
            "whatever",
            vec![set_proof.try_into().unwrap()],
            vec![],
            99999,
        );

        let verified_txs = txs_proofs
            .verify()
            .expect("Valid txs proofs should verify itself");

        assert_eq!(expected, verified_txs);
    }

    #[test]
    fn verify_invalid_proofs() {
        let set_proof = CardanoTransactionsSetProof::new(
            vec!["invalid1".to_string()],
            MKProof::from_leaves(&["invalid2"]).unwrap(),
        );
        let txs_proofs = CardanoTransactionsProofsMessage::new(
            "whatever",
            vec![set_proof.try_into().unwrap()],
            vec![],
            99999,
        );

        let error = txs_proofs
            .verify()
            .expect_err("Invalid txs proofs should fail to verify itself");

        assert!(
            matches!(
                error,
                VerifyCardanoTransactionsProofsError::InvalidSetProof { .. },
            ),
            "Expected 'InvalidSetProof' error but got '{:?}'",
            error
        );
    }

    #[test]
    fn verify_valid_proof_with_different_merkle_root_fail() {
        let set_proofs = vec![
            CardanoTransactionsSetProof::new(
                vec!["tx-1".to_string()],
                MKProof::from_leaves(&["tx-1"]).unwrap(),
            ),
            CardanoTransactionsSetProof::new(
                vec!["tx-2".to_string()],
                MKProof::from_leaves(&["tx-2"]).unwrap(),
            ),
        ];
        let txs_proofs = CardanoTransactionsProofsMessage::new(
            "whatever",
            set_proofs
                .into_iter()
                .map(|p| p.try_into().unwrap())
                .collect(),
            vec![],
            99999,
        );

        let error = txs_proofs
            .verify()
            .expect_err("Txs proofs with non matching merkle root should fail to verify itself");

        assert!(
            matches!(
                error,
                VerifyCardanoTransactionsProofsError::NonMatchingMerkleRoot { .. },
            ),
            "Expected 'NonMatchingMerkleRoot' error but got '{:?}'",
            error
        );
    }

    #[tokio::test]
    async fn verify_hashes_from_verified_cardano_transaction_and_from_signable_builder_are_equals()
    {
        let transaction_1 = CardanoTransaction::new("tx-hash-123", 1, 1);
        let transaction_2 = CardanoTransaction::new("tx-hash-456", 2, 1);
        let transaction_3 = CardanoTransaction::new("tx-hash-789", 3, 1);
        let transaction_4 = CardanoTransaction::new("tx-hash-abc", 4, 1);

        let transactions = vec![transaction_1, transaction_2, transaction_3, transaction_4];
        let transactions_hashes = transactions
            .iter()
            .map(|t| t.transaction_hash.clone())
            .collect::<Vec<_>>();
        let latest_immutable_file_number = 99999;

        let message = {
            let proof = MKProof::from_leaves(&transactions).unwrap();
            let set_proof = CardanoTransactionsSetProof::new(transactions_hashes, proof);

            let verified_transactions_fake = VerifiedCardanoTransactions {
                certificate_hash: "whatever".to_string(),
                merkle_root: set_proof.merkle_root(),
                certified_transactions: set_proof.transactions_hashes().to_vec(),
                latest_immutable_file_number,
            };

            let mut message = ProtocolMessage::new();
            verified_transactions_fake.fill_protocol_message(&mut message);
            message
        };

        let from_signable_builder = {
            let mut mock_transaction_store = MockTransactionStore::new();
            mock_transaction_store
                .expect_store_transactions()
                .returning(|_| Ok(()));

            let cardano_transaction_signable_builder = CardanoTransactionsSignableBuilder::new(
                Arc::new(DumbTransactionParser::new(transactions.clone())),
                Arc::new(mock_transaction_store),
                Path::new("/tmp"),
                Logger::root(slog::Discard, slog::o!()),
            );
            cardano_transaction_signable_builder
                .compute_protocol_message(Beacon {
                    immutable_file_number: latest_immutable_file_number,
                    ..Beacon::default()
                })
                .await
                .unwrap()
        };

        assert!(message.compute_hash() == from_signable_builder.compute_hash());
    }
}

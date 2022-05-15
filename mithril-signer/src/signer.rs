use thiserror::Error;

use mithril_common::crypto_helper::key_encode_hex;
use mithril_common::entities::{self, Beacon};
use mithril_common::fake_data;

use super::certificate_handler::CertificateHandler;
use super::single_signer::SingleSigner;

pub struct Signer {
    certificate_handler: Box<dyn CertificateHandler>,
    single_signer: Box<dyn SingleSigner>,
    current_beacon: Option<Beacon>,
}

#[derive(Error, Debug, PartialEq)]
pub enum SignerError {
    #[error("single signatures computation failed: `{0}`")]
    SingleSignaturesComputeFailed(String),
    #[error("could not retrieve pending certificate: `{0}`")]
    RetrievePendingCertificateFailed(String),
    #[error("could not retrieve protocol initializer")]
    RetrieveProtocolInitializerFailed(),
    #[error("register signer failed: `{0}`")]
    RegisterSignerFailed(String),
    #[error("codec error:`{0}`")]
    Codec(String),
}

impl Signer {
    pub fn new(
        certificate_handler: Box<dyn CertificateHandler>,
        single_signer: Box<dyn SingleSigner>,
    ) -> Self {
        Self {
            certificate_handler,
            single_signer,
            current_beacon: None,
        }
    }

    pub async fn run(&mut self) -> Result<(), SignerError> {
        if let Some(pending_certificate) = self
            .certificate_handler
            .retrieve_pending_certificate()
            .await
            .map_err(|e| SignerError::RetrievePendingCertificateFailed(e.to_string()))?
        {
            let message = fake_data::digest(&pending_certificate.beacon);
            let must_register_signature = match &self.current_beacon {
                None => {
                    self.current_beacon = Some(pending_certificate.beacon);
                    true
                }
                Some(beacon) => beacon != &pending_certificate.beacon,
            };

            if must_register_signature {
                let stake_distribution = pending_certificate.signers;
                let signatures = self
                    .single_signer
                    .compute_single_signatures(
                        message,
                        stake_distribution,
                        &pending_certificate.protocol_parameters,
                    )
                    .map_err(|e| SignerError::SingleSignaturesComputeFailed(e.to_string()))?;
                if !signatures.is_empty() {
                    let _ = self
                        .certificate_handler
                        .register_signatures(&signatures)
                        .await;
                }
            }
        }

        let verification_key = self
            .single_signer
            .get_protocol_initializer()
            .ok_or_else(SignerError::RetrieveProtocolInitializerFailed)?
            .verification_key();
        let verification_key = key_encode_hex(verification_key).map_err(SignerError::Codec)?;
        let signer = entities::Signer::new(self.single_signer.get_party_id(), verification_key);
        self.certificate_handler
            .register_signer(&signer)
            .await
            .map_err(|e| SignerError::RegisterSignerFailed(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::certificate_handler::{CertificateHandlerError, MockCertificateHandler};
    use super::super::single_signer::{MockSingleSigner, SingleSignerError};
    use super::*;
    use mithril_common::crypto_helper::tests_setup::*;
    use mithril_common::fake_data;

    #[tokio::test]
    async fn signer_doesnt_sign_when_there_is_no_pending_certificate() {
        let current_signer = &setup_signers(1)[0];
        let party_id = current_signer.clone().0;
        let protocol_initializer = current_signer.4.clone();
        let mut mock_certificate_handler = MockCertificateHandler::new();
        let mut mock_single_signer = MockSingleSigner::new();
        mock_certificate_handler
            .expect_retrieve_pending_certificate()
            .return_once(|| Ok(None));
        mock_certificate_handler
            .expect_register_signer()
            .return_once(|_| Ok(()));
        mock_single_signer
            .expect_compute_single_signatures()
            .never();
        mock_single_signer
            .expect_get_party_id()
            .return_once(move || party_id);
        mock_single_signer
            .expect_get_protocol_initializer()
            .return_once(move || Some(protocol_initializer));

        let mut signer = Signer::new(
            Box::new(mock_certificate_handler),
            Box::new(mock_single_signer),
        );
        assert!(signer.run().await.is_ok());
    }

    #[tokio::test]
    async fn signer_fails_when_pending_certificate_fails() {
        let mut mock_certificate_handler = MockCertificateHandler::new();
        mock_certificate_handler
            .expect_retrieve_pending_certificate()
            .return_once(|| {
                Err(CertificateHandlerError::RemoteServerTechnical(
                    "An Error".to_string(),
                ))
            });

        let mut signer = Signer::new(
            Box::new(mock_certificate_handler),
            Box::new(MockSingleSigner::new()),
        );
        assert_eq!(
            SignerError::RetrievePendingCertificateFailed(
                CertificateHandlerError::RemoteServerTechnical("An Error".to_string()).to_string()
            ),
            signer.run().await.unwrap_err()
        );
    }

    #[tokio::test]
    async fn signer_sign_when_triggered_by_pending_certificate() {
        let current_signer = &setup_signers(1)[0];
        let party_id = current_signer.clone().0;
        let protocol_initializer = current_signer.4.clone();
        let mut mock_certificate_handler = MockCertificateHandler::new();
        let mut mock_single_signer = MockSingleSigner::new();
        let pending_certificate = fake_data::certificate_pending();
        mock_certificate_handler
            .expect_retrieve_pending_certificate()
            .returning(|| Ok(None))
            .once();
        mock_certificate_handler
            .expect_retrieve_pending_certificate()
            .return_once(|| Ok(Some(pending_certificate)));
        mock_certificate_handler
            .expect_register_signer()
            .returning(|_| Ok(()))
            .times(2);
        mock_certificate_handler
            .expect_register_signatures()
            .return_once(|_| Ok(()));
        mock_single_signer
            .expect_compute_single_signatures()
            .return_once(|_, _, _| Ok(fake_data::single_signatures(2)));
        mock_single_signer
            .expect_get_party_id()
            .returning(move || party_id)
            .times(2);
        mock_single_signer
            .expect_get_protocol_initializer()
            .returning(move || Some(protocol_initializer.clone()))
            .times(2);

        let mut signer = Signer::new(
            Box::new(mock_certificate_handler),
            Box::new(mock_single_signer),
        );
        assert!(signer.run().await.is_ok());
        assert!(signer.run().await.is_ok());
    }

    #[tokio::test]
    async fn signer_sign_only_once_if_pending_certificate_has_not_changed() {
        let current_signer = &setup_signers(1)[0];
        let party_id = current_signer.clone().0;
        let protocol_initializer = current_signer.4.clone();
        let mut mock_certificate_handler = MockCertificateHandler::new();
        let mut mock_single_signer = MockSingleSigner::new();
        let pending_certificate = fake_data::certificate_pending();
        mock_certificate_handler
            .expect_retrieve_pending_certificate()
            .returning(move || Ok(Some(pending_certificate.clone())))
            .times(2);
        mock_certificate_handler
            .expect_register_signatures()
            .return_once(|_| Ok(()));
        mock_certificate_handler
            .expect_register_signer()
            .returning(|_| Ok(()))
            .times(2);
        mock_single_signer
            .expect_compute_single_signatures()
            .return_once(|_, _, _| Ok(fake_data::single_signatures(2)));
        mock_single_signer
            .expect_get_party_id()
            .returning(move || party_id)
            .times(2);
        mock_single_signer
            .expect_get_protocol_initializer()
            .returning(move || Some(protocol_initializer.clone()))
            .times(2);

        let mut signer = Signer::new(
            Box::new(mock_certificate_handler),
            Box::new(mock_single_signer),
        );
        assert!(signer.run().await.is_ok());
        assert!(signer.run().await.is_ok());
    }

    #[tokio::test]
    async fn signer_does_not_send_signatures_if_none_are_computed() {
        let current_signer = &setup_signers(1)[0];
        let party_id = current_signer.clone().0;
        let protocol_initializer = current_signer.4.clone();
        let mut mock_certificate_handler = MockCertificateHandler::new();
        let mut mock_single_signer = MockSingleSigner::new();
        let pending_certificate = fake_data::certificate_pending();
        mock_certificate_handler
            .expect_retrieve_pending_certificate()
            .return_once(|| Ok(Some(pending_certificate)));
        mock_certificate_handler
            .expect_register_signatures()
            .never();
        mock_certificate_handler
            .expect_register_signer()
            .return_once(|_| Ok(()));
        mock_single_signer
            .expect_compute_single_signatures()
            .return_once(|_, _, _| Ok(fake_data::single_signatures(0)));
        mock_single_signer
            .expect_get_party_id()
            .return_once(move || party_id);
        mock_single_signer
            .expect_get_protocol_initializer()
            .return_once(move || Some(protocol_initializer));

        let mut signer = Signer::new(
            Box::new(mock_certificate_handler),
            Box::new(mock_single_signer),
        );
        assert!(signer.run().await.is_ok());
    }

    #[tokio::test]
    async fn signer_fails_if_signature_computation_fails() {
        let mut mock_certificate_handler = MockCertificateHandler::new();
        let mut mock_single_signer = MockSingleSigner::new();
        let pending_certificate = fake_data::certificate_pending();
        mock_certificate_handler
            .expect_retrieve_pending_certificate()
            .return_once(|| Ok(Some(pending_certificate)));
        mock_single_signer
            .expect_compute_single_signatures()
            .return_once(|_, _, _| Err(SingleSignerError::UnregisteredVerificationKey()));

        let mut signer = Signer::new(
            Box::new(mock_certificate_handler),
            Box::new(mock_single_signer),
        );
        assert_eq!(
            SignerError::SingleSignaturesComputeFailed(
                SingleSignerError::UnregisteredVerificationKey().to_string()
            ),
            signer.run().await.unwrap_err()
        );
    }

    #[tokio::test]
    async fn signer_fails_when_register_signer_fails() {
        let current_signer = &setup_signers(1)[0];
        let party_id = current_signer.clone().0;
        let protocol_initializer = current_signer.4.clone();
        let mut mock_certificate_handler = MockCertificateHandler::new();
        let mut mock_single_signer = MockSingleSigner::new();
        mock_certificate_handler
            .expect_retrieve_pending_certificate()
            .return_once(|| Ok(None));
        mock_certificate_handler
            .expect_register_signer()
            .return_once(|_| {
                Err(CertificateHandlerError::RemoteServerLogical(
                    "an error occurred".to_string(),
                ))
            });
        mock_single_signer
            .expect_compute_single_signatures()
            .never();
        mock_single_signer
            .expect_get_party_id()
            .return_once(move || party_id);
        mock_single_signer
            .expect_get_protocol_initializer()
            .return_once(move || Some(protocol_initializer));

        let mut signer = Signer::new(
            Box::new(mock_certificate_handler),
            Box::new(mock_single_signer),
        );
        assert_eq!(
            SignerError::RegisterSignerFailed(
                CertificateHandlerError::RemoteServerLogical("an error occurred".to_string(),)
                    .to_string()
            ),
            signer.run().await.unwrap_err()
        );
    }
}

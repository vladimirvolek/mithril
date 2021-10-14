//! General API for producing proofs from statements and witnesses

/// An environment or context that can contain any long-lived information
/// relevant to the proof backend
pub trait ProverEnv {
    type ProvingKey;
    type VerificationKey;

    fn setup(&self) -> (Self::ProvingKey, Self::VerificationKey);
}

/// Implementors of `Proof<E,S,R,W>` know how to prove that
/// a relation of type `R` holds between values of types `S` and `W`
/// (generally the proofs are knowledge of such a `W`)
pub trait Proof<Env, Statement, Relation, Witness>: Sized
where
    Env: ProverEnv,
{
    fn prove(
        env: &Env,
        pk: &Env::ProvingKey,
        rel: &Relation,
        stmt: &Statement,
        witness: Witness,
    ) -> Option<Self>;

    fn verify(
        &self,
        env: &Env,
        vk: &Env::VerificationKey,
        rel: &Relation,
        stmt: &Statement,
    ) -> bool;
}

pub mod trivial {
    //! A trivial implementation of `Proof` where proofs of knowledge of
    //! witnesses are just the witnesses themselves.
    use super::*;

    #[derive(Debug, Clone)]
    pub struct TrivialEnv;

    #[derive(Debug, Clone)]
    pub struct TrivialProof<W>(pub W);

    impl ProverEnv for TrivialEnv {
        type VerificationKey = ();
        type ProvingKey = ();
        fn setup(&self) -> ((), ()) {
            ((), ())
        }
    }

    impl<Stmt, R, Witness> Proof<TrivialEnv, Stmt, R, Witness> for TrivialProof<Witness>
    where
        R: Fn(&Stmt, &Witness) -> bool,
    {
        fn prove(
            env: &TrivialEnv,
            _pk: &(),
            rel: &R,
            stmt: &Stmt,
            witness: Witness,
        ) -> Option<Self> {
            if rel(stmt, &witness) {
                Some(TrivialProof(witness))
            } else {
                None
            }
        }

        fn verify(&self, env: &TrivialEnv, vk: &(), rel: &R, statement: &Stmt) -> bool {
            rel(statement, &self.0)
        }
    }
}
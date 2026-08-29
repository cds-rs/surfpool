//! The transaction lifecycle machine: the states one transaction moves
//! through between submission and finality, and the single gate every
//! move passes through.
//!
//! The state set is the real node's, extended with what a node holds
//! internally: a client watching a signature sees nothing, then
//! processed, confirmed, finalized; the machine also names admitted
//! (accepted, not yet executed) and the two terminal outcomes a client
//! infers rather than observes (rejected before the ledger, expired in
//! the queue). An execution error is payload on `Processed`, never a
//! state: a failed execution still lands on the ledger.
//!
//! What varies between a real node and a test engine is when and
//! whether an edge fires, never the state set; the spec's table
//! (`spec` module) carries those knobs as a policy column on each
//! accepting edge. The machine here decides only legality.

use serde::{Deserialize, Serialize};

/// The lifecycle states, in the order a transaction visits them.
///
#[doc = include_str!(concat!(env!("OUT_DIR"), "/transaction-lifecycle.rustdoc.md"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransactionLifecycleState {
    /// Arrived, no admission decision yet. Reads by signature return
    /// nothing here, and in every state until `Processed`.
    #[default]
    Unknown,
    /// Passed the admission checks and queued for execution.
    Admitted,
    /// Refused before the ledger, at admission or during
    /// pre-execution work; reported to the submitter and to no one
    /// else. Terminal.
    Rejected,
    /// Executed and on the ledger, successfully or with an error
    /// payload.
    Processed,
    /// Included in a confirmed block.
    Confirmed,
    /// Past the finalization threshold. Terminal.
    Finalized,
    /// Accepted, then aged out of the blockhash window before
    /// execution; the signature never resolves. Terminal.
    Expired,
}

/// The transition alphabet: every move a caller can attempt on the
/// machine. The named methods on [`TransactionLifecycle`] are wrappers
/// that pass one of these through [`TransactionLifecycle::apply`], so
/// a refusal can carry exactly what was attempted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionTransition {
    /// See [`TransactionLifecycle::admit`].
    Admit,
    /// See [`TransactionLifecycle::reject`].
    Reject,
    /// See [`TransactionLifecycle::execute`].
    Execute,
    /// See [`TransactionLifecycle::expire`].
    Expire,
    /// See [`TransactionLifecycle::confirm`].
    Confirm,
    /// See [`TransactionLifecycle::finalize`].
    Finalize,
}

impl TransactionTransition {
    pub const ALL: [TransactionTransition; 6] = [
        TransactionTransition::Admit,
        TransactionTransition::Reject,
        TransactionTransition::Execute,
        TransactionTransition::Expire,
        TransactionTransition::Confirm,
        TransactionTransition::Finalize,
    ];
}

/// A refused transition: what was attempted, and the state it was
/// refused from. The fields are private so the only constructor is the
/// machine's refusal in [`TransactionLifecycle::apply`]; a value of
/// this type is therefore always a refusal that actually happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionLifecycleError {
    attempted: TransactionTransition,
    from: TransactionLifecycleState,
}

/// Why the machine refused a transition, derived from the (transition,
/// state) pair by [`TransactionLifecycleError::kind`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionLifecycleErrorKind {
    /// The lifecycle has already finished, in one of its three
    /// terminal states.
    AlreadyTerminal { state: TransactionLifecycleState },
    /// Admission happens at most once.
    AlreadyAdmitted { state: TransactionLifecycleState },
    /// The transaction is on the ledger; execution, rejection, and
    /// expiry are all behind it.
    AlreadyExecuted { state: TransactionLifecycleState },
    /// Confirmation happens at most once.
    AlreadyConfirmed,
    /// Nothing moves toward the ledger before admission.
    NotAdmitted,
    /// Commitment cannot progress before execution.
    NotExecuted,
    /// Finality follows confirmation.
    NotConfirmed,
}

impl TransactionLifecycleError {
    /// What was attempted.
    pub fn attempted(&self) -> &TransactionTransition {
        &self.attempted
    }

    /// The state the transition was refused from.
    pub fn refused_from(&self) -> TransactionLifecycleState {
        self.from
    }

    /// Classifies the refusal. The classification exists here and
    /// nowhere else, so it cannot drift between call sites, and the
    /// pair remains available as evidence when the name is not enough.
    /// A finished lifecycle dominates every other explanation, so
    /// `AlreadyTerminal` is a complete "stop retrying" discriminator.
    pub fn kind(&self) -> TransactionLifecycleErrorKind {
        use TransactionLifecycleErrorKind as K;
        use TransactionLifecycleState as S;
        use TransactionTransition as T;
        if matches!(self.from, S::Rejected | S::Finalized | S::Expired) {
            return K::AlreadyTerminal { state: self.from };
        }
        match (self.attempted, self.from) {
            (T::Admit, S::Admitted | S::Processed | S::Confirmed) => {
                K::AlreadyAdmitted { state: self.from }
            }
            (T::Reject | T::Execute | T::Expire, S::Processed | S::Confirmed) => {
                K::AlreadyExecuted { state: self.from }
            }
            (T::Execute | T::Expire, S::Unknown) => K::NotAdmitted,
            (T::Confirm | T::Finalize, S::Unknown | S::Admitted) => K::NotExecuted,
            (T::Confirm, S::Confirmed) => K::AlreadyConfirmed,
            (T::Finalize, S::Processed) => K::NotConfirmed,
            (attempted, from) => {
                unreachable!("a legal transition was never refused: {attempted:?} from {from:?}")
            }
        }
    }
}

/// One transaction's lifecycle. The machine holds only the state; the
/// signature, the error payload, and the slots of each commitment step
/// live with the registry entry that owns this value.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TransactionLifecycle {
    state: TransactionLifecycleState,
}

/// Rehydration: a lifecycle resumed at a persisted state. The state
/// itself is the product of prior gated transitions, so resuming there
/// is continuation, never a bypass; every further move still passes
/// through [`TransactionLifecycle::apply`].
impl From<TransactionLifecycleState> for TransactionLifecycle {
    fn from(state: TransactionLifecycleState) -> Self {
        Self { state }
    }
}

impl TransactionLifecycle {
    /// A lifecycle at its start: the transaction has arrived and
    /// nothing has been decided.
    pub fn new() -> Self {
        Self::default()
    }

    /// The current state.
    pub fn state(&self) -> TransactionLifecycleState {
        self.state
    }

    /// The single gate: moves the state where the lifecycle admits the
    /// transition, and refuses otherwise, leaving the state untouched.
    pub fn apply(
        &mut self,
        transition: TransactionTransition,
    ) -> Result<(), TransactionLifecycleError> {
        use TransactionLifecycleState as S;
        use TransactionTransition as T;
        // The accepting moves, and nothing else; every unlisted pair is
        // a refusal. The spec's table lists each refusal decided, and
        // the conformance sweep holds this encoding equal to it.
        let next = match (self.state, transition) {
            (S::Unknown, T::Admit) => S::Admitted,
            (S::Unknown, T::Reject) => S::Rejected,
            (S::Admitted, T::Reject) => S::Rejected,
            (S::Admitted, T::Execute) => S::Processed,
            (S::Admitted, T::Expire) => S::Expired,
            (S::Processed, T::Confirm) => S::Confirmed,
            (S::Confirmed, T::Finalize) => S::Finalized,
            _ => {
                return Err(TransactionLifecycleError {
                    attempted: transition,
                    from: self.state,
                });
            }
        };
        self.state = next;
        Ok(())
    }

    /// Records the admission decision's accepting side: the checks
    /// passed and the transaction is queued for execution.
    pub fn admit(&mut self) -> Result<(), TransactionLifecycleError> {
        self.apply(TransactionTransition::Admit)
    }

    /// Records a refusal before the ledger, at admission or during
    /// pre-execution work. Terminal.
    pub fn reject(&mut self) -> Result<(), TransactionLifecycleError> {
        self.apply(TransactionTransition::Reject)
    }

    /// Records execution: the transaction is on the ledger, with any
    /// error carried as payload by the registry entry.
    pub fn execute(&mut self) -> Result<(), TransactionLifecycleError> {
        self.apply(TransactionTransition::Execute)
    }

    /// Records expiry in the queue: the blockhash aged out before
    /// execution. Terminal.
    pub fn expire(&mut self) -> Result<(), TransactionLifecycleError> {
        self.apply(TransactionTransition::Expire)
    }

    /// Records inclusion in a confirmed block.
    pub fn confirm(&mut self) -> Result<(), TransactionLifecycleError> {
        self.apply(TransactionTransition::Confirm)
    }

    /// Records passage of the finalization threshold. Terminal.
    pub fn finalize(&mut self) -> Result<(), TransactionLifecycleError> {
        self.apply(TransactionTransition::Finalize)
    }
}

#[cfg(test)]
mod spec;

#[cfg(test)]
mod conformance_tests;

#[cfg(test)]
mod spec_doc_tests;

//! The spec: the transaction lifecycle's rules stated on their own.
//!
//! The principle carries over from the startup spec: spec and
//! implementation must be different encodings of the same rules. The
//! conformance sweep proves the machine agrees with this module, and
//! that proof is empty the moment the two share code. Everything here
//! is written in the spec's vocabulary and reads the machine only
//! through its public accessors.
//!
//! The states are the real node's: a client watching a signature sees
//! nothing, then processed, confirmed, finalized. The spec adds the
//! states a node holds internally (admitted) and the terminal outcomes
//! a client infers rather than observes (rejected, expired). What
//! varies between a real node and a test engine is never the state
//! set: it is whether and when an edge fires, so each accepting row
//! carries a `policy` column naming the knobs that gate it. A refusal
//! is never policy; the table's Refuse cells hold in every
//! configuration.

use super::*;

/// A cell's successor: the machine moves to a state, or refuses the
/// transition, leaving the state untouched and the caller with an
/// error.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Next {
    To(TransactionLifecycleState),
    Refuse,
}

/// One cell of the transition table. Listing every (state, transition)
/// pair, refusals included, keeps the lookup total and shows each
/// refusal decided rather than defaulted; `the_table_is_total` asserts
/// the listing is complete. `policy` names the knobs that gate an
/// accepting edge; it is empty for unguarded edges and for refusals.
pub struct Row {
    pub state: TransactionLifecycleState,
    pub transition: TransactionTransition,
    pub next: Next,
    pub policy: &'static str,
}

const fn row(
    state: TransactionLifecycleState,
    transition: TransactionTransition,
    next: Next,
    policy: &'static str,
) -> Row {
    Row {
        state,
        transition,
        next,
        policy,
    }
}

use Next::{Refuse, To};
use TransactionLifecycleState as S;
use TransactionTransition as T;

/// Every state, in the order the spec's table lists them. The first
/// listed state is the machine's default.
pub const STATES: [TransactionLifecycleState; 7] = [
    S::Unknown,
    S::Admitted,
    S::Rejected,
    S::Processed,
    S::Confirmed,
    S::Finalized,
    S::Expired,
];

/// The lifecycle as data: every (state, transition) cell, refusals
/// included.
#[rustfmt::skip]
pub const TABLE: &[Row] = &[
    //   state         transition   next              policy
    row( S::Unknown,   T::Admit,    To(S::Admitted),
         "sigverify unless skip_sig_verify; recency unless skip_blockhash_check; \
          simulation unless skip_preflight; a durable nonce selects ValidateAtExecution" ),
    row( S::Unknown,   T::Reject,   To(S::Rejected),
         "the failing side of the admission checks" ),
    row( S::Unknown,   T::Execute,  Refuse, "" ), // every execution follows an admission
    row( S::Unknown,   T::Expire,   Refuse, "" ), // nothing to expire before admission
    row( S::Unknown,   T::Confirm,  Refuse, "" ),
    row( S::Unknown,   T::Finalize, Refuse, "" ),

    row( S::Admitted,  T::Admit,    Refuse, "" ), // admitted at most once
    row( S::Admitted,  T::Reject,   To(S::Rejected),
         "a pre-execution engine failure: account fetch, ALT resolution" ),
    row( S::Admitted,  T::Execute,  To(S::Processed),
         "an execution error is payload, not state; a failed execution still lands" ),
    row( S::Admitted,  T::Expire,   To(S::Expired),
         "ValidateAtExecution only; recency validated at admission cannot expire" ),
    row( S::Admitted,  T::Confirm,  Refuse, "" ), // cannot confirm before executing
    row( S::Admitted,  T::Finalize, Refuse, "" ),

    row( S::Rejected,  T::Admit,    Refuse, "" ), // terminal
    row( S::Rejected,  T::Reject,   Refuse, "" ),
    row( S::Rejected,  T::Execute,  Refuse, "" ),
    row( S::Rejected,  T::Expire,   Refuse, "" ),
    row( S::Rejected,  T::Confirm,  Refuse, "" ),
    row( S::Rejected,  T::Finalize, Refuse, "" ),

    row( S::Processed, T::Admit,    Refuse, "" ), // admitted at most once
    row( S::Processed, T::Reject,   Refuse, "" ), // on the ledger; too late to refuse
    row( S::Processed, T::Execute,  Refuse, "" ), // executed at most once
    row( S::Processed, T::Expire,   Refuse, "" ), // executed before the window closed
    row( S::Processed, T::Confirm,  To(S::Confirmed),
         "slot cadence: the next block after execution" ),
    row( S::Processed, T::Finalize, Refuse, "" ), // finality follows confirmation

    row( S::Confirmed, T::Admit,    Refuse, "" ),
    row( S::Confirmed, T::Reject,   Refuse, "" ),
    row( S::Confirmed, T::Execute,  Refuse, "" ),
    row( S::Confirmed, T::Expire,   Refuse, "" ),
    row( S::Confirmed, T::Confirm,  Refuse, "" ), // confirmed at most once
    row( S::Confirmed, T::Finalize, To(S::Finalized),
         "slot at least the confirmation slot plus FINALIZATION_SLOT_THRESHOLD" ),

    row( S::Finalized, T::Admit,    Refuse, "" ), // terminal
    row( S::Finalized, T::Reject,   Refuse, "" ),
    row( S::Finalized, T::Execute,  Refuse, "" ),
    row( S::Finalized, T::Expire,   Refuse, "" ),
    row( S::Finalized, T::Confirm,  Refuse, "" ),
    row( S::Finalized, T::Finalize, Refuse, "" ),

    row( S::Expired,   T::Admit,    Refuse, "" ), // terminal; the signature never resolves
    row( S::Expired,   T::Reject,   Refuse, "" ),
    row( S::Expired,   T::Execute,  Refuse, "" ),
    row( S::Expired,   T::Expire,   Refuse, "" ),
    row( S::Expired,   T::Confirm,  Refuse, "" ),
    row( S::Expired,   T::Finalize, Refuse, "" ),
];

/// The table's answer for (state, transition): the state the
/// transition moves the lifecycle to, or `None` where the table
/// refuses the move.
pub fn transition(
    from: TransactionLifecycleState,
    transition: TransactionTransition,
) -> Option<TransactionLifecycleState> {
    let cell = TABLE
        .iter()
        .find(|row| row.state == from && row.transition == transition)
        .expect("the_table_is_total guarantees every cell exists");
    match cell.next {
        To(state) => Some(state),
        Refuse => None,
    }
}

/// The table's name for a state.
pub fn state_name(state: TransactionLifecycleState) -> &'static str {
    match state {
        S::Unknown => "Unknown",
        S::Admitted => "Admitted",
        S::Rejected => "Rejected",
        S::Processed => "Processed",
        S::Confirmed => "Confirmed",
        S::Finalized => "Finalized",
        S::Expired => "Expired",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_total() {
        for state in STATES {
            for transition in TransactionTransition::ALL {
                let count = TABLE
                    .iter()
                    .filter(|row| row.state == state && row.transition == transition)
                    .count();
                assert_eq!(
                    count,
                    1,
                    "the cell ({}, {:?}) must appear exactly once",
                    state_name(state),
                    transition
                );
            }
        }
        assert_eq!(
            TABLE.len(),
            STATES.len() * TransactionTransition::ALL.len()
        );
    }

    /// Policy annotates accepting edges only: a refusal that held only
    /// under some configuration would make the table's Refuse cells
    /// conditional, and they are not.
    #[test]
    fn policy_annotates_accepting_edges_only() {
        for row in TABLE {
            if row.next == Refuse {
                assert!(
                    row.policy.is_empty(),
                    "the refusal ({}, {:?}) carries a policy",
                    state_name(row.state),
                    row.transition
                );
            }
        }
    }
}

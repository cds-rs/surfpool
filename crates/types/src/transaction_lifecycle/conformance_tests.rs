//! The conformance sweep: the machine and the spec table are
//! independent encodings of the transition decision, and this module
//! holds them equal on every (state, transition) cell.

use super::{spec, *};

/// A machine driven to the given state along its canonical path. The
/// machine is memoryless beyond its state, so one path per state
/// suffices for the sweep; alternate entries (a rejection after
/// admission, for one) are cells the sweep itself visits.
fn machine_in(state: TransactionLifecycleState) -> TransactionLifecycle {
    use TransactionTransition as T;
    let path: &[TransactionTransition] = match state {
        TransactionLifecycleState::Unknown => &[],
        TransactionLifecycleState::Admitted => &[T::Admit],
        TransactionLifecycleState::Rejected => &[T::Reject],
        TransactionLifecycleState::Processed => &[T::Admit, T::Execute],
        TransactionLifecycleState::Confirmed => &[T::Admit, T::Execute, T::Confirm],
        TransactionLifecycleState::Finalized => &[T::Admit, T::Execute, T::Confirm, T::Finalize],
        TransactionLifecycleState::Expired => &[T::Admit, T::Expire],
    };
    let mut machine = TransactionLifecycle::new();
    for &transition in path {
        machine
            .apply(transition)
            .unwrap_or_else(|error| panic!("the canonical path to {state:?} broke: {error:?}"));
    }
    assert_eq!(machine.state(), state, "the canonical path ends elsewhere");
    machine
}

#[test]
fn a_new_lifecycle_is_unknown() {
    assert_eq!(
        TransactionLifecycle::new().state(),
        TransactionLifecycleState::Unknown
    );
}

#[test]
fn the_machine_agrees_with_the_spec_on_every_cell() {
    for state in spec::STATES {
        for transition in TransactionTransition::ALL {
            let mut machine = machine_in(state);
            let outcome = machine.apply(transition);
            match spec::transition(state, transition) {
                Some(next) => {
                    outcome.unwrap_or_else(|error| {
                        panic!(
                            "the spec accepts ({}, {transition:?}) but the machine \
                             refused: {error:?}",
                            spec::state_name(state)
                        )
                    });
                    assert_eq!(machine.state(), next);
                }
                None => {
                    let error = outcome.expect_err(&format!(
                        "the spec refuses ({}, {transition:?}) but the machine moved",
                        spec::state_name(state)
                    ));
                    assert_eq!(machine.state(), state, "a refusal must not move the state");
                    assert_eq!(*error.attempted(), transition);
                    assert_eq!(error.refused_from(), state);
                }
            }
        }
    }
}

#[test]
fn the_happy_path_reaches_finalized() {
    let mut machine = TransactionLifecycle::new();
    machine.admit().unwrap();
    machine.execute().unwrap();
    machine.confirm().unwrap();
    machine.finalize().unwrap();
    assert_eq!(machine.state(), TransactionLifecycleState::Finalized);
}

#[test]
fn expiry_exists_only_between_admission_and_execution() {
    let mut admitted = machine_in(TransactionLifecycleState::Admitted);
    admitted.expire().unwrap();
    assert_eq!(admitted.state(), TransactionLifecycleState::Expired);

    let mut unknown = TransactionLifecycle::new();
    unknown.expire().unwrap_err();

    let mut processed = machine_in(TransactionLifecycleState::Processed);
    processed.expire().unwrap_err();
}

#[test]
fn refusal_kinds_name_the_reason() {
    use TransactionLifecycleErrorKind as K;

    let terminal = machine_in(TransactionLifecycleState::Finalized)
        .apply(TransactionTransition::Admit)
        .unwrap_err();
    assert_eq!(
        terminal.kind(),
        K::AlreadyTerminal {
            state: TransactionLifecycleState::Finalized
        }
    );

    let unadmitted = TransactionLifecycle::new()
        .apply(TransactionTransition::Execute)
        .unwrap_err();
    assert_eq!(unadmitted.kind(), K::NotAdmitted);

    let readmitted = machine_in(TransactionLifecycleState::Processed)
        .apply(TransactionTransition::Admit)
        .unwrap_err();
    assert_eq!(
        readmitted.kind(),
        K::AlreadyAdmitted {
            state: TransactionLifecycleState::Processed
        }
    );

    let reexecuted = machine_in(TransactionLifecycleState::Confirmed)
        .apply(TransactionTransition::Execute)
        .unwrap_err();
    assert_eq!(
        reexecuted.kind(),
        K::AlreadyExecuted {
            state: TransactionLifecycleState::Confirmed
        }
    );

    let unexecuted = machine_in(TransactionLifecycleState::Admitted)
        .apply(TransactionTransition::Confirm)
        .unwrap_err();
    assert_eq!(unexecuted.kind(), K::NotExecuted);

    let unconfirmed = machine_in(TransactionLifecycleState::Processed)
        .apply(TransactionTransition::Finalize)
        .unwrap_err();
    assert_eq!(unconfirmed.kind(), K::NotConfirmed);

    let reconfirmed = machine_in(TransactionLifecycleState::Confirmed)
        .apply(TransactionTransition::Confirm)
        .unwrap_err();
    assert_eq!(
        reconfirmed.kind(),
        K::AlreadyConfirmed
    );
}

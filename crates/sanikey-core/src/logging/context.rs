//! Dynamically scoped session propagation for synchronous instrumented code.

use std::cell::RefCell;
use std::sync::Arc;

use super::{Operation, Session};

struct ActiveContext {
    session: Session,
    operation_id: Option<u64>,
}

thread_local! {
    static CURRENT: RefCell<Vec<ActiveContext>> = const { RefCell::new(Vec::new()) };
}

struct ScopeGuard;

/// Restores the previous thread-local session when a scope exits.
impl Drop for ScopeGuard {
    fn drop(&mut self) {
        CURRENT.with(|sessions| {
            sessions.borrow_mut().pop();
        });
    }
}

/// Installs and retrieves the session associated with the current thread.
impl Session {
    /// Runs synchronous code with this session available to instrumentation.
    ///
    /// Nested scopes restore the previous session, including during unwinding.
    /// A newly spawned thread does not inherit the scope: pass a cloned session
    /// to the worker and enter its scope explicitly.
    pub fn scoped<R>(&self, action: impl FnOnce() -> R) -> R {
        self.scoped_with_operation(None, action)
    }

    pub(super) fn scoped_with_operation<R>(
        &self,
        operation_id: Option<u64>,
        action: impl FnOnce() -> R,
    ) -> R {
        CURRENT.with(|sessions| {
            sessions.borrow_mut().push(ActiveContext {
                session: self.clone(),
                operation_id,
            })
        });
        let _guard = ScopeGuard;
        action()
    }

    /// Returns the current thread's innermost scoped session, if any.
    #[must_use]
    pub fn current() -> Option<Self> {
        CURRENT.with(|sessions| {
            sessions
                .borrow()
                .last()
                .map(|active| active.session.clone())
        })
    }
}

/// Runs an operation with its session available in the current thread.
impl Operation {
    /// Runs code with this operation's session and identifier as context.
    pub fn scoped<R>(&self, action: impl FnOnce() -> R) -> R {
        self.session.scoped_with_operation(Some(self.id), action)
    }
}

pub(super) fn current_operation_id(session: &Session) -> Option<u64> {
    CURRENT.with(|sessions| {
        sessions
            .borrow()
            .iter()
            .rev()
            .find(|active| Arc::ptr_eq(&active.session.state, &session.state))
            .and_then(|active| active.operation_id)
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::Session;
    use crate::logging::{Component, Level};

    #[test]
    fn scopes_restore_after_nesting_and_unwinding_and_are_not_inherited() {
        let directory = std::env::temp_dir().join(format!(
            "sanikey-context-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (outer, outer_path) =
            Session::create_in_directory_with_level(&directory, Component::Core, Level::Events)
                .unwrap();
        let (inner, inner_path) =
            Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
                .unwrap();
        assert!(Session::current().is_none());
        outer.scoped(|| {
            assert!(!Session::current().unwrap().trace_enabled());
            let result = std::panic::catch_unwind(|| {
                inner.scoped(|| {
                    assert!(Session::current().unwrap().trace_enabled());
                    assert!(
                        std::thread::spawn(|| Session::current().is_none())
                            .join()
                            .unwrap()
                    );
                    panic!("synthetic failure");
                })
            });
            assert!(result.is_err());
            assert!(!Session::current().unwrap().trace_enabled());
        });
        assert!(Session::current().is_none());
        drop((outer, inner));
        fs::remove_file(outer_path).unwrap();
        fs::remove_file(inner_path).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}

//! Property-test strategies for identifiers. Enabled by the `test-support` feature.

use proptest::prelude::*;

use crate::NamespacedId;

/// A valid namespace or name.
pub fn valid_part() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,63}"
}

pub fn namespaced_id() -> impl Strategy<Value = NamespacedId> {
    (valid_part(), valid_part()).prop_map(|(namespace, name)| {
        NamespacedId::new(&namespace, &name).expect("strategy only generates valid parts")
    })
}

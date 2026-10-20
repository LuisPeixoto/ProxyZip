//! OpenAPI 3.0 and 3.1 support: turn a specification into a mock server, and check captured traffic against it.

mod example;
mod schema;
mod spec;
mod validate;

pub use example::generate_example;
pub use spec::{Operation, Spec};
pub use validate::{Report, Violation, ViolationKind, validate_flows};

//! Replay suites: an ordered list of requests with variables, assertions and values passed from one to the next.
//!
//! A suite is a `.pzsuite` JSON file. [`run`] sends its requests (optionally through a running ProxyZip, so they show up
//! in the list) and returns a [`Report`] that can be printed, saved as JSON or as JUnit XML.

mod model;
mod postman;
mod report;
mod runner;

pub use model::{Assertion, Extract, FORMAT, Suite, SuiteRequest, VERSION};
pub use postman::import_postman;
pub use report::{AssertionResult, Report, RequestResult, to_junit};
pub use runner::{RunOptions, run};

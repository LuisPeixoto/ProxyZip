//! jq filters over JSON bodies, powered by jaq (a jq implementation in Rust).

use jaq_core::load::{Arena, File, Loader};
use jaq_core::{Compiler, Ctx, Vars, data, unwrap_valr};
use jaq_json::{Val, read};
use serde_json::Value;

/// Upper bound on results so a runaway filter such as `repeat(1)` cannot exhaust memory.
const MAX_RESULTS: usize = 10_000;

/// Run `filter` over `input` and return every result.
pub fn run(input: &[u8], filter: &str) -> Result<Vec<Value>, String> {
    let input = read::parse_single(input).map_err(|e| format!("the body is not valid JSON: {e}"))?;
    let program = File { code: filter, path: () };

    let defs = jaq_core::defs().chain(jaq_std::defs()).chain(jaq_json::defs());
    let funs = jaq_core::funs().chain(jaq_std::funs()).chain(jaq_json::funs());
    let loader = Loader::new(defs);
    let arena = Arena::default();
    let modules = loader.load(&arena, program).map_err(|e| format!("invalid filter: {e:?}"))?;
    let compiled =
        Compiler::default().with_funs(funs).compile(modules).map_err(|e| format!("invalid filter: {e:?}"))?;

    let ctx = Ctx::<data::JustLut<Val>>::new(&compiled.lut, Vars::new([]));
    let mut results = Vec::new();
    for item in compiled.id.run((ctx, input)).map(unwrap_valr).take(MAX_RESULTS) {
        let value = item.map_err(|e| format!("{e}"))?;
        let text = value.to_string();
        results.push(serde_json::from_str(&text).unwrap_or(Value::String(text)));
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const DOC: &[u8] = br#"{"users":[{"name":"ada","age":36},{"name":"alan","age":41}],"total":2}"#;

    #[test]
    fn paths_iteration_and_pipes() {
        assert_eq!(run(DOC, ".total").unwrap(), vec![json!(2)]);
        assert_eq!(run(DOC, ".users[].name").unwrap(), vec![json!("ada"), json!("alan")]);
        assert_eq!(run(DOC, ".users | map(.age) | add").unwrap(), vec![json!(77)]);
        assert_eq!(run(DOC, ".users[] | select(.age > 40) | .name").unwrap(), vec![json!("alan")]);
    }

    #[test]
    fn construction_and_builtins() {
        assert_eq!(run(DOC, "{n: .total, first: .users[0].name}").unwrap(), vec![json!({"n": 2, "first": "ada"})]);
        assert_eq!(run(DOC, ".users | length").unwrap(), vec![json!(2)]);
        assert_eq!(run(DOC, ".users[0].name | ascii_upcase").unwrap(), vec![json!("ADA")]);
        assert_eq!(run(DOC, "keys").unwrap(), vec![json!(["total", "users"])]);
    }

    #[test]
    fn errors_are_reported() {
        assert!(run(DOC, ".users[").unwrap_err().contains("invalid filter"));
        assert!(run(b"not json", ".").unwrap_err().contains("not valid JSON"));
        assert!(run(DOC, ".total | keys").is_err(), "runtime type errors surface too");
    }

    #[test]
    fn runaway_filters_are_capped() {
        assert_eq!(run(b"1", "repeat(1)").unwrap().len(), MAX_RESULTS);
    }
}

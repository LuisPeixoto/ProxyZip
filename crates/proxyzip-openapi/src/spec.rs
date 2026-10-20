//! Loading a specification and listing its operations.

use anyhow::{Context, Result, bail};
use proxyzip_rules::{MockBody, MockRoute, MockServer};
use serde_json::Value;

use crate::example::generate_example;
use crate::schema::resolve;

const METHODS: [&str; 7] = ["get", "put", "post", "delete", "options", "head", "patch"];

#[derive(Debug, Clone)]
pub struct Spec {
    doc: Value,
}

/// One operation (method + path) of the specification.
#[derive(Debug, Clone)]
pub struct Operation {
    pub method: String,
    /// The path as written, e.g. `/pets/{petId}`.
    pub path: String,
    pub value: Value,
    /// Parameters of the path item and of the operation, with `$ref`s resolved.
    pub parameters: Vec<Value>,
}

impl Operation {
    pub fn label(&self) -> String {
        match self.value.get("operationId").and_then(Value::as_str) {
            Some(id) => format!("{} {} ({id})", self.method, self.path),
            None => format!("{} {}", self.method, self.path),
        }
    }
}

impl Spec {
    /// Parse JSON or YAML.
    pub fn parse(text: &str) -> Result<Spec> {
        let doc: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => {
                serde_yaml_ng::from_str(text).context("the specification is neither valid JSON nor valid YAML")?
            }
        };
        let version = doc.get("openapi").and_then(Value::as_str).unwrap_or("");
        if !version.starts_with("3.") {
            bail!("this is not an OpenAPI 3 document (missing `openapi: 3.x`)");
        }
        if !doc.get("paths").is_some_and(Value::is_object) {
            bail!("the specification has no `paths`");
        }
        Ok(Spec { doc })
    }

    pub fn title(&self) -> String {
        self.doc.pointer("/info/title").and_then(Value::as_str).unwrap_or("API").to_string()
    }

    pub fn doc(&self) -> &Value {
        &self.doc
    }

    /// The URLs in `servers`, as `(host, base path)`. Empty when the specification names none.
    pub fn servers(&self) -> Vec<(String, String)> {
        self.doc
            .get("servers")
            .and_then(Value::as_array)
            .map(|servers| {
                servers
                    .iter()
                    .filter_map(|s| s.get("url").and_then(Value::as_str))
                    .filter_map(|u| url::Url::parse(u).ok())
                    .filter_map(|u| Some((u.host_str()?.to_string(), u.path().trim_end_matches('/').to_string())))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn operations(&self) -> Vec<Operation> {
        let mut out = Vec::new();
        let Some(paths) = self.doc.get("paths").and_then(Value::as_object) else { return out };
        for (path, item) in paths {
            let item = resolve(&self.doc, item);
            for method in METHODS {
                let Some(op) = item.get(method) else { continue };
                let op = resolve(&self.doc, op);
                let mut parameters: Vec<Value> = Vec::new();
                for p in item
                    .get("parameters")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .chain(op.get("parameters").and_then(Value::as_array).into_iter().flatten())
                {
                    let p = resolve(&self.doc, p);
                    // An operation's parameter overrides the path item's with the same name and location.
                    let key = (p.get("name").cloned(), p.get("in").cloned());
                    parameters.retain(|q| (q.get("name").cloned(), q.get("in").cloned()) != key);
                    parameters.push(p);
                }
                out.push(Operation { method: method.to_ascii_uppercase(), path: path.clone(), value: op, parameters });
            }
        }
        out
    }

    /// A mock server that answers every operation with its documented example (or one generated from the schema).
    /// `host` is the host the mock should capture; leave it empty to use the mock only on its own port.
    pub fn to_mock_server(&self, name: &str, host: &str, listen_port: Option<u16>) -> MockServer {
        let mut routes: Vec<MockRoute> = self.operations().iter().map(|op| self.route_for(op)).collect();
        // Literal paths must win over `:param` paths that would also match.
        routes.sort_by_key(|r| (r.path.matches(':').count(), std::cmp::Reverse(r.path.len())));
        MockServer {
            name: name.to_string(),
            enabled: true,
            host: host.to_string(),
            listen_port,
            passthrough: false,
            routes,
        }
    }

    fn route_for(&self, op: &Operation) -> MockRoute {
        let (status, response) = self.success_response(op);
        let json = response.as_ref().and_then(|r| r.pointer("/content/application~1json")).cloned();
        let body = match &json {
            Some(media) => self.example_of(media),
            None => None,
        };
        let mut headers = Vec::new();
        let mut text = String::new();
        if let Some(value) = body {
            headers.push(("content-type".to_string(), "application/json".to_string()));
            text = serde_json::to_string_pretty(&value).unwrap_or_default();
        }
        MockRoute {
            name: op.value.get("operationId").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| op.label()),
            enabled: true,
            method: op.method.clone(),
            path: to_route_path(&op.path),
            status,
            headers,
            body: MockBody::Inline { text },
            template: false,
            delay_ms: 0,
        }
    }

    /// The status and response object of the first 2xx response (or the first one declared).
    pub fn success_response(&self, op: &Operation) -> (u16, Option<Value>) {
        let Some(responses) = op.value.get("responses").and_then(Value::as_object) else { return (200, None) };
        let mut codes: Vec<&String> = responses.keys().collect();
        codes.sort();
        let pick = codes
            .iter()
            .find(|c| c.starts_with('2') && c.parse::<u16>().is_ok())
            .or_else(|| codes.iter().find(|c| c.parse::<u16>().is_ok()))
            .map(|c| (*c).clone());
        match pick {
            Some(code) => (code.parse().unwrap_or(200), responses.get(&code).map(|r| resolve(&self.doc, r))),
            None => (200, responses.get("default").map(|r| resolve(&self.doc, r))),
        }
    }

    /// The `example`, the first of `examples`, or a value generated from the schema of a media type object.
    fn example_of(&self, media: &Value) -> Option<Value> {
        if let Some(e) = media.get("example") {
            return Some(e.clone());
        }
        if let Some(examples) = media.get("examples").and_then(Value::as_object)
            && let Some(first) = examples.values().next()
        {
            let first = resolve(&self.doc, first);
            if let Some(v) = first.get("value") {
                return Some(v.clone());
            }
        }
        media.get("schema").map(|s| generate_example(&self.doc, s))
    }
}

/// `/pets/{petId}` → `/pets/:petId`.
pub fn to_route_path(path: &str) -> String {
    let mut out = String::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        match rest[start..].find('}') {
            Some(end) => {
                out.push(':');
                out.push_str(&rest[start + 1..start + end]);
                rest = &rest[start + end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const PETSTORE: &str = include_str!("../tests/fixtures/petstore.yaml");
    pub const ORDERS: &str = include_str!("../tests/fixtures/orders-3.1.json");

    #[test]
    fn parses_yaml_and_json_and_rejects_the_rest() {
        let pets = Spec::parse(PETSTORE).unwrap();
        assert_eq!(pets.title(), "Petstore");
        assert_eq!(pets.servers(), vec![("petstore.example.com".to_string(), "/v1".to_string())]);
        assert_eq!(pets.operations().len(), 4);
        let orders = Spec::parse(ORDERS).unwrap();
        assert_eq!(orders.operations().len(), 1);
        assert!(Spec::parse("swagger: '2.0'\npaths: {}").is_err());
        assert!(Spec::parse("openapi: 3.0.0").is_err());
        assert!(Spec::parse("{ not: [valid").is_err());
    }

    #[test]
    fn merges_path_and_operation_parameters() {
        let ops = Spec::parse(PETSTORE).unwrap().operations();
        let get = ops.iter().find(|o| o.method == "GET" && o.path == "/pets/{petId}").unwrap();
        let names: Vec<_> = get.parameters.iter().map(|p| p["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["petId", "X-Trace"]);
        assert!(get.label().contains("getPet"));
    }

    #[test]
    fn route_paths() {
        assert_eq!(to_route_path("/pets/{petId}/photos/{n}"), "/pets/:petId/photos/:n");
        assert_eq!(to_route_path("/plain"), "/plain");
        assert_eq!(to_route_path("/odd/{open"), "/odd/{open");
    }

    #[test]
    fn mock_server_uses_examples_then_generated_values() {
        let server = Spec::parse(PETSTORE).unwrap().to_mock_server("pets", "petstore.example.com", None);
        assert_eq!(server.routes.len(), 4);
        let by = |name: &str| server.routes.iter().find(|r| r.name == name).unwrap();
        let MockBody::Inline { text } = &by("getPet").body else { panic!() };
        assert_eq!(
            serde_json::from_str::<Value>(text).unwrap(),
            serde_json::json!({ "id": 7, "name": "Rex", "tag": "dog" })
        );
        assert_eq!(by("getPet").path, "/pets/:petId");
        assert_eq!(by("createPet").status, 201);
        let MockBody::Inline { text } = &by("listPets").body else { panic!() };
        let list: Value = serde_json::from_str(text).unwrap();
        assert!(list.is_array() && list[0]["name"].is_string(), "generated from the Pet schema: {list}");
        let health = server.routes.iter().find(|r| r.path == "/health").unwrap();
        assert_eq!(health.status, 204);
        assert!(health.headers.is_empty() && matches!(&health.body, MockBody::Inline { text } if text.is_empty()));
    }

    #[test]
    fn literal_routes_sort_before_parameterised_ones() {
        let spec = Spec::parse(
            "openapi: 3.0.0\ninfo: {title: t, version: '1'}\npaths:\n  /users/{id}:\n    get: {responses: {'200': {description: ok}}}\n  /users/me:\n    get: {responses: {'200': {description: ok}}}\n",
        )
        .unwrap();
        let routes = spec.to_mock_server("x", "h", None).routes;
        assert_eq!(routes[0].path, "/users/me");
        assert_eq!(routes[1].path, "/users/:id");
    }
}

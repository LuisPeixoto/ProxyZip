//! Turn a captured request into code that sends it again.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use proxyzip_core::Flow;

/// Languages and tools that [`generate`] can target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Curl,
    Httpie,
    PowerShell,
    Python,
    JavaScriptFetch,
    NodeAxios,
    Go,
    Rust,
    Java,
    Kotlin,
    Swift,
    CSharp,
    Php,
    Ruby,
}

impl Language {
    pub const ALL: [Language; 14] = [
        Language::Curl,
        Language::Httpie,
        Language::PowerShell,
        Language::Python,
        Language::JavaScriptFetch,
        Language::NodeAxios,
        Language::Go,
        Language::Rust,
        Language::Java,
        Language::Kotlin,
        Language::Swift,
        Language::CSharp,
        Language::Php,
        Language::Ruby,
    ];

    /// Stable identifier used by the app and the command line.
    pub fn id(self) -> &'static str {
        match self {
            Language::Curl => "curl",
            Language::Httpie => "httpie",
            Language::PowerShell => "powershell",
            Language::Python => "python",
            Language::JavaScriptFetch => "javascript",
            Language::NodeAxios => "node-axios",
            Language::Go => "go",
            Language::Rust => "rust",
            Language::Java => "java",
            Language::Kotlin => "kotlin",
            Language::Swift => "swift",
            Language::CSharp => "csharp",
            Language::Php => "php",
            Language::Ruby => "ruby",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Language::Curl => "cURL",
            Language::Httpie => "HTTPie",
            Language::PowerShell => "PowerShell",
            Language::Python => "Python (requests)",
            Language::JavaScriptFetch => "JavaScript (fetch)",
            Language::NodeAxios => "Node.js (axios)",
            Language::Go => "Go (net/http)",
            Language::Rust => "Rust (reqwest)",
            Language::Java => "Java (HttpClient)",
            Language::Kotlin => "Kotlin (OkHttp)",
            Language::Swift => "Swift (URLSession)",
            Language::CSharp => "C# (HttpClient)",
            Language::Php => "PHP (cURL)",
            Language::Ruby => "Ruby (Net::HTTP)",
        }
    }

    /// Syntax highlighting hint for editors: `shell`, `python`, `javascript`, …
    pub fn syntax(self) -> &'static str {
        match self {
            Language::Curl | Language::Httpie => "shell",
            Language::PowerShell => "powershell",
            Language::Python => "python",
            Language::JavaScriptFetch | Language::NodeAxios => "javascript",
            Language::Go => "go",
            Language::Rust => "rust",
            Language::Java => "java",
            Language::Kotlin => "kotlin",
            Language::Swift => "swift",
            Language::CSharp => "csharp",
            Language::Php => "php",
            Language::Ruby => "ruby",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.id().eq_ignore_ascii_case(id))
    }
}

/// Headers a client library sets itself, which would only get in the way if copied.
const SKIPPED: &[&str] =
    &["host", "content-length", "connection", "proxy-connection", "accept-encoding", "transfer-encoding"];

/// What the generators work from.
struct Prepared {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    content_type: Option<String>,
    body: Body,
}

enum Body {
    None,
    Text(String),
    Binary(Vec<u8>),
}

fn prepare(flow: &Flow) -> Prepared {
    let req = &flow.request;
    let (decoded, encodings, error) = proxyzip_codec::decode_content(&req.headers, &req.body);
    let was_decoded = !encodings.is_empty() && error.is_none();
    let headers: Vec<(String, String)> = req
        .headers
        .iter()
        .filter(|(k, _)| !k.starts_with(':') && !SKIPPED.contains(&k.to_ascii_lowercase().as_str()))
        .filter(|(k, _)| !(was_decoded && k.eq_ignore_ascii_case("content-encoding")))
        .cloned()
        .collect();
    let content_type = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v.clone());
    let body = if decoded.is_empty() {
        Body::None
    } else {
        match std::str::from_utf8(&decoded) {
            Ok(text) => Body::Text(text.to_string()),
            Err(_) => Body::Binary(decoded.to_vec()),
        }
    };
    Prepared { method: req.method.to_ascii_uppercase(), url: req.url(), headers, content_type, body }
}

/// Quote a string for a language. JSON-style escapes work in most of them; the flags cover the exceptions.
fn quote(s: &str, dollar: bool, brace_unicode: bool, hash: bool) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    let mut previous = '\0';
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '$' if dollar => out.push_str("\\$"),
            '{' if hash && previous == '#' => out.push_str("\\{"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                if brace_unicode {
                    out.push_str(&format!("\\u{{{:x}}}", c as u32));
                } else {
                    out.push_str(&format!("\\u{:04x}", c as u32));
                }
            }
            c => out.push(c),
        }
        previous = c;
    }
    out.push('"');
    out
}

/// Double-quoted, JSON-style (Python, JavaScript, Java, C#, Go).
fn q(s: &str) -> String {
    quote(s, false, false, false)
}

/// Double-quoted for languages that interpolate `$` (Kotlin).
fn q_dollar(s: &str) -> String {
    quote(s, true, false, false)
}

/// Double-quoted with `\u{…}` escapes (Rust, Swift).
fn q_braced(s: &str) -> String {
    quote(s, false, true, false)
}

/// Single-quoted where only `\` and `'` are special (PHP, Ruby).
fn q_single(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// Single-quoted PowerShell: a quote is doubled.
fn q_powershell(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn b64(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

/// Generate source code for `flow`'s request.
pub fn generate(flow: &Flow, language: Language) -> String {
    let p = prepare(flow);
    match language {
        Language::Curl => crate::curl::to_curl(flow),
        Language::Httpie => httpie(&p),
        Language::PowerShell => powershell(&p),
        Language::Python => python(&p),
        Language::JavaScriptFetch => fetch(&p),
        Language::NodeAxios => axios(&p),
        Language::Go => go(&p),
        Language::Rust => rust(&p),
        Language::Java => java(&p),
        Language::Kotlin => kotlin(&p),
        Language::Swift => swift(&p),
        Language::CSharp => csharp(&p),
        Language::Php => php(&p),
        Language::Ruby => ruby(&p),
    }
}

fn httpie(p: &Prepared) -> String {
    let sq = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
    let mut parts = Vec::new();
    let mut prefix = String::new();
    match &p.body {
        Body::Text(t) => parts.push(format!("--raw {}", sq(t))),
        // HTTPie reads a body from stdin when it is piped.
        Body::Binary(b) => prefix = format!("printf %s {} | base64 --decode | ", sq(&b64(b))),
        Body::None => {}
    }
    parts.insert(0, "http".to_string());
    parts.push(p.method.clone());
    parts.push(sq(&p.url));
    for (k, v) in &p.headers {
        parts.push(sq(&format!("{k}:{v}")));
    }
    format!("{prefix}{}", parts.join(" \\\n  "))
}

fn powershell(p: &Prepared) -> String {
    let mut out = String::new();
    let others: Vec<_> = p.headers.iter().filter(|(k, _)| !k.eq_ignore_ascii_case("content-type")).collect();
    if !others.is_empty() {
        out.push_str("$headers = @{\n");
        for (k, v) in &others {
            out.push_str(&format!("  {} = {}\n", q_powershell(k), q_powershell(v)));
        }
        out.push_str("}\n\n");
    }
    out.push_str(&format!("Invoke-WebRequest -Uri {} -Method {}", q_powershell(&p.url), p.method));
    if !others.is_empty() {
        out.push_str(" -Headers $headers");
    }
    if let Some(ct) = &p.content_type {
        out.push_str(&format!(" -ContentType {}", q_powershell(ct)));
    }
    match &p.body {
        Body::Text(t) => out.push_str(&format!(" -Body {}", q_powershell(t))),
        Body::Binary(b) => out.push_str(&format!(" -Body ([Convert]::FromBase64String({}))", q_powershell(&b64(b)))),
        Body::None => {}
    }
    out.push('\n');
    out
}

fn python(p: &Prepared) -> String {
    let mut out = String::new();
    if matches!(p.body, Body::Binary(_)) {
        out.push_str("import base64\n");
    }
    out.push_str("import requests\n\n");
    out.push_str(&format!("response = requests.request(\n    {},\n    {},\n", q(&p.method), q(&p.url)));
    if !p.headers.is_empty() {
        out.push_str("    headers={\n");
        for (k, v) in &p.headers {
            out.push_str(&format!("        {}: {},\n", q(k), q(v)));
        }
        out.push_str("    },\n");
    }
    match &p.body {
        Body::Text(t) => out.push_str(&format!("    data={}.encode(\"utf-8\"),\n", q(t))),
        Body::Binary(b) => out.push_str(&format!("    data=base64.b64decode({}),\n", q(&b64(b)))),
        Body::None => {}
    }
    out.push_str(")\nprint(response.status_code)\nprint(response.text)\n");
    out
}

fn js_body(p: &Prepared) -> Option<String> {
    match &p.body {
        Body::Text(t) => Some(q(t)),
        Body::Binary(b) => Some(format!("Uint8Array.from(atob({}), (c) => c.charCodeAt(0))", q(&b64(b)))),
        Body::None => None,
    }
}

fn fetch(p: &Prepared) -> String {
    let mut out = format!("const response = await fetch({}, {{\n  method: {},\n", q(&p.url), q(&p.method));
    if !p.headers.is_empty() {
        out.push_str("  headers: {\n");
        for (k, v) in &p.headers {
            out.push_str(&format!("    {}: {},\n", q(k), q(v)));
        }
        out.push_str("  },\n");
    }
    if let Some(body) = js_body(p) {
        out.push_str(&format!("  body: {body},\n"));
    }
    out.push_str("});\nconsole.log(response.status, await response.text());\n");
    out
}

fn axios(p: &Prepared) -> String {
    let mut out = format!(
        "const axios = require(\"axios\");\n\nconst response = await axios.request({{\n  method: {},\n  url: {},\n",
        q(&p.method.to_ascii_lowercase()),
        q(&p.url)
    );
    if !p.headers.is_empty() {
        out.push_str("  headers: {\n");
        for (k, v) in &p.headers {
            out.push_str(&format!("    {}: {},\n", q(k), q(v)));
        }
        out.push_str("  },\n");
    }
    match &p.body {
        Body::Text(t) => out.push_str(&format!("  data: {},\n", q(t))),
        Body::Binary(b) => out.push_str(&format!("  data: Buffer.from({}, \"base64\"),\n", q(&b64(b)))),
        Body::None => {}
    }
    out.push_str("  transformResponse: (data) => data,\n  validateStatus: () => true,\n});\nconsole.log(response.status, response.data);\n");
    out
}

fn go(p: &Prepared) -> String {
    let binary = matches!(p.body, Body::Binary(_));
    let has_body = !matches!(p.body, Body::None);
    let mut imports = vec!["\"fmt\"", "\"io\"", "\"net/http\""];
    if binary {
        imports.insert(0, "\"bytes\"");
        imports.insert(1, "\"encoding/base64\"");
    } else if has_body {
        imports.push("\"strings\"");
    }
    imports.sort_by_key(|i| i.trim_matches('"').to_string());
    let mut out = String::from("package main\n\nimport (\n");
    for i in &imports {
        out.push_str(&format!("\t{i}\n"));
    }
    out.push_str(")\n\nfunc main() {\n");
    let body_arg = match &p.body {
        Body::Text(t) => {
            out.push_str(&format!("\tbody := strings.NewReader({})\n", q(t)));
            "body"
        }
        Body::Binary(b) => {
            out.push_str(&format!(
                "\tdata, _ := base64.StdEncoding.DecodeString({})\n\tbody := bytes.NewReader(data)\n",
                q(&b64(b))
            ));
            "body"
        }
        Body::None => "nil",
    };
    out.push_str(&format!(
        "\treq, err := http.NewRequest({}, {}, {body_arg})\n\tif err != nil {{\n\t\tpanic(err)\n\t}}\n",
        q(&p.method),
        q(&p.url)
    ));
    for (k, v) in &p.headers {
        out.push_str(&format!("\treq.Header.Set({}, {})\n", q(k), q(v)));
    }
    out.push_str("\n\tresp, err := http.DefaultClient.Do(req)\n\tif err != nil {\n\t\tpanic(err)\n\t}\n\tdefer resp.Body.Close()\n\n\tdata2, _ := io.ReadAll(resp.Body)\n\tfmt.Println(resp.StatusCode, string(data2))\n}\n");
    out
}

fn rust(p: &Prepared) -> String {
    let mut out = String::from("let client = reqwest::Client::new();\nlet response = client\n");
    let standard = matches!(p.method.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD");
    if standard {
        out.push_str(&format!("    .{}({})\n", p.method.to_ascii_lowercase(), q_braced(&p.url)));
    } else {
        out.push_str(&format!(
            "    .request(reqwest::Method::from_bytes(b{}).unwrap(), {})\n",
            q_braced(&p.method),
            q_braced(&p.url)
        ));
    }
    for (k, v) in &p.headers {
        out.push_str(&format!("    .header({}, {})\n", q_braced(k), q_braced(v)));
    }
    match &p.body {
        Body::Text(t) => out.push_str(&format!("    .body({})\n", q_braced(t))),
        Body::Binary(b) => out.push_str(&format!(
            "    // needs the `base64` crate: use base64::Engine as _;\n    .body(base64::engine::general_purpose::STANDARD.decode({}).unwrap())\n",
            q_braced(&b64(b))
        )),
        Body::None => {}
    }
    out.push_str("    .send()\n    .await?;\nprintln!(\"{} {}\", response.status(), response.text().await?);\n");
    out
}

fn java(p: &Prepared) -> String {
    let publisher = match &p.body {
        Body::Text(t) => format!("HttpRequest.BodyPublishers.ofString({})", q(t)),
        Body::Binary(b) => {
            format!("HttpRequest.BodyPublishers.ofByteArray(java.util.Base64.getDecoder().decode({}))", q(&b64(b)))
        }
        Body::None => "HttpRequest.BodyPublishers.noBody()".into(),
    };
    let mut out = String::from(
        "HttpClient client = HttpClient.newHttpClient();\nHttpRequest request = HttpRequest.newBuilder()\n",
    );
    out.push_str(&format!("    .uri(URI.create({}))\n    .method({}, {publisher})\n", q(&p.url), q(&p.method)));
    for (k, v) in &p.headers {
        out.push_str(&format!("    .header({}, {})\n", q(k), q(v)));
    }
    out.push_str("    .build();\nHttpResponse<String> response = client.send(request, HttpResponse.BodyHandlers.ofString());\nSystem.out.println(response.statusCode() + \" \" + response.body());\n");
    out
}

fn kotlin(p: &Prepared) -> String {
    let media =
        p.content_type.as_ref().map(|c| format!("{}.toMediaType()", q_dollar(c))).unwrap_or_else(|| "null".into());
    let body = match &p.body {
        Body::Text(t) => format!("{}.toRequestBody({media})", q_dollar(t)),
        Body::Binary(b) => {
            format!("java.util.Base64.getDecoder().decode({}).toRequestBody({media})", q_dollar(&b64(b)))
        }
        Body::None if matches!(p.method.as_str(), "POST" | "PUT" | "PATCH") => format!("\"\".toRequestBody({media})"),
        Body::None => "null".into(),
    };
    let mut out = String::from("val client = OkHttpClient()\nval request = Request.Builder()\n");
    out.push_str(&format!("    .url({})\n    .method({}, {body})\n", q_dollar(&p.url), q_dollar(&p.method)));
    for (k, v) in &p.headers {
        if !matches!(p.body, Body::None) && k.eq_ignore_ascii_case("content-type") {
            continue; // OkHttp takes it from the request body's media type
        }
        out.push_str(&format!("    .addHeader({}, {})\n", q_dollar(k), q_dollar(v)));
    }
    out.push_str("    .build()\nclient.newCall(request).execute().use { response ->\n    println(\"${response.code} ${response.body?.string()}\")\n}\n");
    out
}

fn swift(p: &Prepared) -> String {
    let mut out = format!(
        "import Foundation\n\nvar request = URLRequest(url: URL(string: {})!)\nrequest.httpMethod = {}\n",
        q_braced(&p.url),
        q_braced(&p.method)
    );
    for (k, v) in &p.headers {
        out.push_str(&format!("request.setValue({}, forHTTPHeaderField: {})\n", q_braced(v), q_braced(k)));
    }
    match &p.body {
        Body::Text(t) => out.push_str(&format!("request.httpBody = {}.data(using: .utf8)\n", q_braced(t))),
        Body::Binary(b) => out.push_str(&format!("request.httpBody = Data(base64Encoded: {})\n", q_braced(&b64(b)))),
        Body::None => {}
    }
    out.push_str("\nlet (data, response) = try await URLSession.shared.data(for: request)\nprint((response as! HTTPURLResponse).statusCode, String(decoding: data, as: UTF8.self))\n");
    out
}

fn csharp(p: &Prepared) -> String {
    let mut out = format!(
        "using var client = new HttpClient();\nusing var request = new HttpRequestMessage(new HttpMethod({}), {});\n",
        q(&p.method),
        q(&p.url)
    );
    let has_body = !matches!(p.body, Body::None);
    for (k, v) in &p.headers {
        if has_body && k.eq_ignore_ascii_case("content-type") {
            continue; // set on the content
        }
        out.push_str(&format!("request.Headers.TryAddWithoutValidation({}, {});\n", q(k), q(v)));
    }
    let media = p.content_type.as_ref().map(|c| c.split(';').next().unwrap_or(c).trim().to_string());
    match &p.body {
        Body::Text(t) => {
            out.push_str(&format!("request.Content = new StringContent({}, Encoding.UTF8", q(t)));
            if let Some(m) = &media {
                out.push_str(&format!(", {}", q(m)));
            }
            out.push_str(");\n");
        }
        Body::Binary(b) => {
            out.push_str(&format!(
                "request.Content = new ByteArrayContent(Convert.FromBase64String({}));\n",
                q(&b64(b))
            ));
            if let Some(m) = &media {
                out.push_str(&format!("request.Content.Headers.ContentType = new MediaTypeHeaderValue({});\n", q(m)));
            }
        }
        Body::None => {}
    }
    out.push_str("var response = await client.SendAsync(request);\nConsole.WriteLine($\"{(int)response.StatusCode} {await response.Content.ReadAsStringAsync()}\");\n");
    out
}

fn php(p: &Prepared) -> String {
    let mut out = format!(
        "$ch = curl_init({});\ncurl_setopt_array($ch, [\n    CURLOPT_CUSTOMREQUEST => {},\n    CURLOPT_RETURNTRANSFER => true,\n",
        q_single(&p.url),
        q_single(&p.method)
    );
    if !p.headers.is_empty() {
        out.push_str("    CURLOPT_HTTPHEADER => [\n");
        for (k, v) in &p.headers {
            out.push_str(&format!("        {},\n", q_single(&format!("{k}: {v}"))));
        }
        out.push_str("    ],\n");
    }
    match &p.body {
        Body::Text(t) => out.push_str(&format!("    CURLOPT_POSTFIELDS => {},\n", q_single(t))),
        Body::Binary(b) => out.push_str(&format!("    CURLOPT_POSTFIELDS => base64_decode({}),\n", q_single(&b64(b)))),
        Body::None => {}
    }
    out.push_str("]);\n$response = curl_exec($ch);\necho curl_getinfo($ch, CURLINFO_HTTP_CODE), \" \", $response, \"\\n\";\ncurl_close($ch);\n");
    out
}

fn ruby(p: &Prepared) -> String {
    let has_body = !matches!(p.body, Body::None);
    let class = match p.method.as_str() {
        "GET" => Some("Get"),
        "POST" => Some("Post"),
        "PUT" => Some("Put"),
        "PATCH" => Some("Patch"),
        "DELETE" => Some("Delete"),
        "HEAD" => Some("Head"),
        "OPTIONS" => Some("Options"),
        _ => None,
    };
    let mut out = String::from("require 'net/http'\nrequire 'uri'\n");
    if matches!(p.body, Body::Binary(_)) {
        out.push_str("require 'base64'\n");
    }
    out.push_str(&format!(
        "\nuri = URI({})\nhttp = Net::HTTP.new(uri.host, uri.port)\nhttp.use_ssl = uri.scheme == 'https'\n",
        q_single(&p.url)
    ));
    match class {
        Some(c) => out.push_str(&format!("request = Net::HTTP::{c}.new(uri)\n")),
        None => out.push_str(&format!(
            "request = Net::HTTPGenericRequest.new({}, {has_body}, true, uri)\n",
            q_single(&p.method)
        )),
    }
    for (k, v) in &p.headers {
        out.push_str(&format!("request[{}] = {}\n", q_single(k), q_single(v)));
    }
    match &p.body {
        Body::Text(t) => out.push_str(&format!("request.body = {}\n", q_single(t))),
        Body::Binary(b) => out.push_str(&format!("request.body = Base64.decode64({})\n", q_single(&b64(b)))),
        Body::None => {}
    }
    out.push_str("\nresponse = http.request(request)\nputs \"#{response.code} #{response.body}\"\n");
    out
}

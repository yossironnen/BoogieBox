//! Subsonic response envelope: one node tree rendered to XML (default), JSON (`f=json`) or JSONP
//! (`f=jsonp&callback=`). Lists are declared explicitly so JSON arrays stay arrays even with one item
//! (clients break when a one-element list arrives as an object).

use axum::{
    body::Body,
    http::{header, StatusCode},
    response::Response,
};
use serde_json::{Map, Value as Json};

/// Subsonic REST API version reported to clients.
pub const API_VERSION: &str = "1.16.1";
/// Server type reported in every response.
pub const SERVER_TYPE: &str = "boogiebox";

/// An attribute value (or a node's text content).
#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl From<&str> for Scalar {
    fn from(v: &str) -> Self {
        Scalar::Str(v.to_owned())
    }
}
impl From<String> for Scalar {
    fn from(v: String) -> Self {
        Scalar::Str(v)
    }
}
impl From<&String> for Scalar {
    fn from(v: &String) -> Self {
        Scalar::Str(v.clone())
    }
}
impl From<i64> for Scalar {
    fn from(v: i64) -> Self {
        Scalar::Int(v)
    }
}
impl From<i32> for Scalar {
    fn from(v: i32) -> Self {
        Scalar::Int(v.into())
    }
}
impl From<u32> for Scalar {
    fn from(v: u32) -> Self {
        Scalar::Int(v.into())
    }
}
impl From<usize> for Scalar {
    fn from(v: usize) -> Self {
        Scalar::Int(i64::try_from(v).unwrap_or(i64::MAX))
    }
}
impl From<f64> for Scalar {
    fn from(v: f64) -> Self {
        Scalar::Float(v)
    }
}
impl From<bool> for Scalar {
    fn from(v: bool) -> Self {
        Scalar::Bool(v)
    }
}

impl Scalar {
    fn to_json(&self) -> Json {
        match self {
            Scalar::Str(s) => Json::String(s.clone()),
            Scalar::Int(n) => Json::from(*n),
            Scalar::Float(f) => serde_json::Number::from_f64(*f)
                .map(Json::Number)
                .unwrap_or(Json::Null),
            Scalar::Bool(b) => Json::Bool(*b),
        }
    }

    fn to_text(&self) -> String {
        match self {
            Scalar::Str(s) => s.clone(),
            Scalar::Int(n) => n.to_string(),
            Scalar::Float(f) => f.to_string(),
            Scalar::Bool(b) => b.to_string(),
        }
    }
}

/// One XML element / JSON object.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Node {
    pub name: &'static str,
    pub attrs: Vec<(&'static str, Scalar)>,
    /// Single child elements (JSON objects).
    pub objects: Vec<Node>,
    /// Repeated child elements (JSON arrays), each item named by the list name.
    pub lists: Vec<(&'static str, Vec<Node>)>,
    /// Text content (`genre`, `folder`, lyrics lines).
    pub text: Option<Scalar>,
}

impl Node {
    pub fn new(name: &'static str) -> Self {
        Node {
            name,
            ..Node::default()
        }
    }

    pub fn attr(mut self, name: &'static str, value: impl Into<Scalar>) -> Self {
        self.attrs.push((name, value.into()));
        self
    }

    pub fn opt<T: Into<Scalar>>(self, name: &'static str, value: Option<T>) -> Self {
        match value {
            Some(v) => self.attr(name, v),
            None => self,
        }
    }

    pub fn object(mut self, child: Node) -> Self {
        self.objects.push(child);
        self
    }

    /// Adds a list; items are renamed to `name` so XML and JSON agree.
    pub fn list(mut self, name: &'static str, items: Vec<Node>) -> Self {
        let items = items
            .into_iter()
            .map(|mut n| {
                n.name = name;
                n
            })
            .collect();
        self.lists.push((name, items));
        self
    }

    pub fn text(mut self, value: impl Into<Scalar>) -> Self {
        self.text = Some(value.into());
        self
    }

    fn to_json(&self) -> Json {
        // A bare text node (`<folder>1</folder>`) is a JSON scalar.
        if self.attrs.is_empty() && self.objects.is_empty() && self.lists.is_empty() {
            if let Some(text) = &self.text {
                return text.to_json();
            }
        }
        let mut map = Map::new();
        for (k, v) in &self.attrs {
            map.insert((*k).to_owned(), v.to_json());
        }
        for child in &self.objects {
            map.insert(child.name.to_owned(), child.to_json());
        }
        for (name, items) in &self.lists {
            if !items.is_empty() {
                map.insert(
                    (*name).to_owned(),
                    Json::Array(items.iter().map(Node::to_json).collect()),
                );
            }
        }
        if let Some(text) = &self.text {
            map.insert("value".to_owned(), text.to_json());
        }
        Json::Object(map)
    }

    fn write_xml(&self, out: &mut String) {
        out.push('<');
        out.push_str(self.name);
        for (k, v) in &self.attrs {
            out.push(' ');
            out.push_str(k);
            out.push_str("=\"");
            escape_xml(&v.to_text(), out);
            out.push('"');
        }
        let has_children =
            !self.objects.is_empty() || self.lists.iter().any(|(_, items)| !items.is_empty());
        if !has_children && self.text.is_none() {
            out.push_str("/>");
            return;
        }
        out.push('>');
        if let Some(text) = &self.text {
            escape_xml(&text.to_text(), out);
        }
        for child in &self.objects {
            child.write_xml(out);
        }
        for (_, items) in &self.lists {
            for item in items {
                item.write_xml(out);
            }
        }
        out.push_str("</");
        out.push_str(self.name);
        out.push('>');
    }
}

/// Escapes XML special characters and drops characters XML 1.0 cannot carry (tags sometimes contain them).
fn escape_xml(input: &str, out: &mut String) {
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
}

/// Requested response format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Format {
    Xml,
    Json,
    Jsonp(String),
}

impl Format {
    /// `f` + `callback` parameters. An invalid JSONP callback name falls back to JSON.
    pub fn from_params(f: Option<&str>, callback: Option<&str>) -> Self {
        match f.map(str::to_ascii_lowercase).as_deref() {
            Some("json") => Format::Json,
            Some("jsonp") => match callback.filter(|c| valid_callback(c)) {
                Some(cb) => Format::Jsonp(cb.to_owned()),
                None => Format::Json,
            },
            _ => Format::Xml,
        }
    }
}

/// JavaScript identifier path (`cb`, `jQuery123.done`), at most 64 characters.
fn valid_callback(cb: &str) -> bool {
    let mut chars = cb.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$');
    first_ok
        && cb.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.')
}

/// Subsonic error codes used by BoogieBox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubsonicError {
    pub code: u32,
    pub message: String,
}

impl SubsonicError {
    pub fn new(code: u32, message: impl Into<String>) -> Self {
        SubsonicError {
            code,
            message: message.into(),
        }
    }
    /// 0 — generic / unsupported.
    pub fn generic(message: impl Into<String>) -> Self {
        Self::new(0, message)
    }
    /// 10 — required parameter missing.
    pub fn missing(param: &str) -> Self {
        Self::new(10, format!("Required parameter is missing: {param}"))
    }
    /// 40 — wrong credentials.
    pub fn wrong_credentials() -> Self {
        Self::new(40, "Wrong username or password")
    }
    /// 50 — not authorized.
    pub fn not_authorized() -> Self {
        Self::new(50, "User is not authorized for the given operation")
    }
    /// 70 — data not found.
    pub fn not_found(what: &str) -> Self {
        Self::new(70, format!("{what} not found"))
    }
    /// 0 — the method exists in the spec but BoogieBox does not offer it.
    pub fn unsupported() -> Self {
        Self::generic("Not supported by BoogieBox")
    }
}

fn envelope(status: &str, server_version: &str) -> Node {
    Node::new("subsonic-response")
        .attr("status", status)
        .attr("version", API_VERSION)
        .attr("type", SERVER_TYPE)
        .attr("serverVersion", server_version)
        .attr("openSubsonic", true)
}

/// Successful response: `payload`'s attributes, objects and lists are merged into the envelope.
pub fn ok_response(format: &Format, payload: Node) -> Response {
    let mut env = envelope("ok", env!("CARGO_PKG_VERSION"));
    env.attrs.extend(payload.attrs);
    env.objects = payload.objects;
    env.lists = payload.lists;
    render(format, &env)
}

/// Failed response (HTTP 200, as the Subsonic spec requires).
pub fn error_response(format: &Format, err: &SubsonicError) -> Response {
    let env = envelope("failed", env!("CARGO_PKG_VERSION")).object(
        Node::new("error")
            .attr("code", err.code)
            .attr("message", err.message.as_str()),
    );
    render(format, &env)
}

fn render(format: &Format, env: &Node) -> Response {
    let (content_type, body) = match format {
        Format::Xml => {
            let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
            let mut root = env.clone();
            root.attrs
                .insert(0, ("xmlns", Scalar::from("http://subsonic.org/restapi")));
            root.write_xml(&mut out);
            ("text/xml; charset=utf-8", out)
        }
        Format::Json => ("application/json", json_body(env).to_string()),
        Format::Jsonp(cb) => (
            "application/javascript",
            format!("{cb}({});", json_body(env)),
        ),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn json_body(env: &Node) -> Json {
    let mut map = Map::new();
    map.insert(env.name.to_owned(), env.to_json());
    Json::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn body_text(resp: Response) -> String {
        String::from_utf8(
            to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap()
    }

    fn sample() -> Node {
        Node::new("payload").object(Node::new("artists").attr("ignoredArticles", "The").list(
            "index",
            vec![Node::new("x").attr("name", "A").list(
                "artist",
                vec![Node::new("x")
                        .attr("id", "a1")
                        .attr("name", "AC/DC & <Co>")
                        .attr("albumCount", 2)],
            )],
        ))
    }

    #[tokio::test]
    async fn renders_xml_with_escaping_and_namespace() {
        let xml = body_text(ok_response(&Format::Xml, sample())).await;
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?><subsonic-response xmlns=\"http://subsonic.org/restapi\" status=\"ok\" version=\"1.16.1\" type=\"boogiebox\""));
        assert!(xml.contains("openSubsonic=\"true\">"));
        assert!(xml.contains(
            "<artists ignoredArticles=\"The\"><index name=\"A\"><artist id=\"a1\" name=\"AC/DC &amp; &lt;Co&gt;\" albumCount=\"2\"/></index></artists>"
        ));
        assert!(xml.ends_with("</subsonic-response>"));
    }

    #[tokio::test]
    async fn renders_json_lists_as_arrays_even_with_one_item() {
        let json: Json =
            serde_json::from_str(&body_text(ok_response(&Format::Json, sample())).await).unwrap();
        let root = &json["subsonic-response"];
        assert_eq!(root["status"], "ok");
        assert_eq!(root["openSubsonic"], true);
        let index = &root["artists"]["index"];
        assert!(index.is_array());
        assert_eq!(index[0]["artist"][0]["albumCount"], 2);
        assert_eq!(index[0]["artist"][0]["name"], "AC/DC & <Co>");
    }

    #[tokio::test]
    async fn text_nodes_render_as_value_or_scalar() {
        let payload = Node::new("p")
            .object(Node::new("genres").list(
                "genre",
                vec![Node::new("g").attr("songCount", 3).text("Rock")],
            ))
            .object(
                Node::new("user")
                    .attr("username", "a")
                    .list("folder", vec![Node::new("f").text(1)]),
            );
        let json: Json =
            serde_json::from_str(&body_text(ok_response(&Format::Json, payload.clone())).await)
                .unwrap();
        assert_eq!(
            json["subsonic-response"]["genres"]["genre"][0]["value"],
            "Rock"
        );
        assert_eq!(json["subsonic-response"]["user"]["folder"][0], 1);
        let xml = body_text(ok_response(&Format::Xml, payload)).await;
        assert!(xml.contains("<genre songCount=\"3\">Rock</genre>"));
        assert!(xml.contains("<folder>1</folder>"));
    }

    #[tokio::test]
    async fn errors_and_jsonp() {
        let resp = error_response(
            &Format::Jsonp("cb.done".into()),
            &SubsonicError::missing("id"),
        );
        assert_eq!(
            resp.headers()[header::CONTENT_TYPE],
            "application/javascript"
        );
        let text = body_text(resp).await;
        assert!(text.starts_with("cb.done({\"subsonic-response\":"));
        assert!(text.contains("\"status\":\"failed\""));
        assert!(text.contains("\"code\":10"));
        assert!(text.ends_with(");"));
        let xml = body_text(error_response(
            &Format::Xml,
            &SubsonicError::wrong_credentials(),
        ))
        .await;
        assert!(xml.contains("status=\"failed\""));
        assert!(xml.contains("<error code=\"40\" message=\"Wrong username or password\"/>"));
    }

    #[test]
    fn format_selection_and_callback_validation() {
        assert_eq!(Format::from_params(None, None), Format::Xml);
        assert_eq!(Format::from_params(Some("JSON"), None), Format::Json);
        assert_eq!(
            Format::from_params(Some("jsonp"), Some("cb_1")),
            Format::Jsonp("cb_1".into())
        );
        assert_eq!(
            Format::from_params(Some("jsonp"), Some("alert(1)")),
            Format::Json
        );
        assert_eq!(
            Format::from_params(Some("jsonp"), Some("1cb")),
            Format::Json
        );
        assert_eq!(Format::from_params(Some("jsonp"), None), Format::Json);
    }

    #[test]
    fn xml_drops_invalid_control_characters() {
        let mut out = String::new();
        escape_xml("a\u{1}b\tc'\"", &mut out);
        assert_eq!(out, "ab\tc&apos;&quot;");
    }
}

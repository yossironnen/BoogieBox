//! Minimal namespace-agnostic XML tree built on `quick-xml`, for the documents renderers send us
//! (device descriptions, SCPDs, SOAP responses, `LastChange`/DIDL metadata).
//!
//! Element names are stored by **local name** (prefix stripped) because vendors disagree on prefixes
//! (`s:Envelope` / `SOAP-ENV:Envelope`, `u:PlayResponse` / `m:PlayResponse`). Entity references are resolved,
//! CDATA is kept as text, comments/PIs/DOCTYPE are ignored. Input size and nesting are bounded.

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use quick_xml::XmlVersion;

use super::UpnpError;

/// Upper bound on accepted XML documents (device descriptions are a few KB; SCPDs ~20 KB).
pub const MAX_XML_BYTES: usize = 512 * 1024;
const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 20_000;

/// One XML element with its local name, attributes, direct text content and child elements.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct XmlNode {
    /// Local element name (namespace prefix removed).
    pub name: String,
    /// Attributes as `(local name, unescaped value)` pairs, in document order.
    pub attrs: Vec<(String, String)>,
    /// Concatenated, unescaped text directly inside this element (not including children's text).
    pub text: String,
    /// Child elements in document order.
    pub children: Vec<XmlNode>,
}

impl XmlNode {
    /// First direct child whose local name matches `name` (ASCII case-insensitive).
    pub fn child(&self, name: &str) -> Option<&XmlNode> {
        self.children
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }

    /// All direct children whose local name matches `name` (ASCII case-insensitive).
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a XmlNode> + 'a {
        self.children
            .iter()
            .filter(move |c| c.name.eq_ignore_ascii_case(name))
    }

    /// Trimmed text of the first direct child named `name`, if present and non-empty.
    pub fn child_text(&self, name: &str) -> Option<String> {
        self.child(name)
            .map(|c| c.text.trim().to_string())
            .filter(|t| !t.is_empty())
    }

    /// Depth-first search (self included) for the first element named `name`.
    pub fn find(&self, name: &str) -> Option<&XmlNode> {
        if self.name.eq_ignore_ascii_case(name) {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(name))
    }

    /// Follows a path of child names from this element, e.g. `["device", "serviceList"]`.
    pub fn path(&self, names: &[&str]) -> Option<&XmlNode> {
        names.iter().try_fold(self, |node, name| node.child(name))
    }

    /// Value of the attribute with local name `name` (ASCII case-insensitive).
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn local(name: &str) -> String {
    match name.rsplit_once(':') {
        Some((_, l)) => l.to_string(),
        None => name.to_string(),
    }
}

fn resolve_entity(name: &str) -> Option<String> {
    let s = match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        _ => {
            let code =
                if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                    u32::from_str_radix(hex, 16).ok()?
                } else {
                    name.strip_prefix('#')?.parse::<u32>().ok()?
                };
            return char::from_u32(code).map(|c| c.to_string());
        }
    };
    Some(s.to_string())
}

fn element_from(start: &quick_xml::events::BytesStart<'_>) -> Result<XmlNode, UpnpError> {
    let mut node = XmlNode {
        name: local(start.name().as_ref()),
        ..XmlNode::default()
    };
    for attr in start.attributes() {
        let attr = attr.map_err(|e| UpnpError::Xml(e.to_string()))?;
        let value = attr
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(|e| UpnpError::Xml(e.to_string()))?;
        node.attrs
            .push((local(attr.key.as_ref()), value.into_owned()));
    }
    Ok(node)
}

/// Parses `xml` into a tree rooted at its document element.
pub fn parse(xml: &str) -> Result<XmlNode, UpnpError> {
    if xml.len() > MAX_XML_BYTES {
        return Err(UpnpError::Xml("document too large".into()));
    }
    // Some devices prepend a UTF-8 BOM or stray whitespace before the declaration.
    let xml = xml.trim_start_matches('\u{feff}').trim_start();
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root: Option<XmlNode> = None;
    let mut nodes = 0usize;

    loop {
        let event = reader
            .read_event()
            .map_err(|e| UpnpError::Xml(format!("at byte {}: {e}", reader.error_position())))?;
        match event {
            Event::Start(start) => {
                nodes += 1;
                if stack.len() >= MAX_DEPTH || nodes > MAX_NODES {
                    return Err(UpnpError::Xml("document too deep or too large".into()));
                }
                stack.push(element_from(&start)?);
            }
            Event::Empty(start) => {
                nodes += 1;
                if nodes > MAX_NODES {
                    return Err(UpnpError::Xml("document too large".into()));
                }
                let node = element_from(&start)?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None if root.is_none() => root = Some(node),
                    None => {}
                }
            }
            Event::End(_) => {
                let node = stack
                    .pop()
                    .ok_or_else(|| UpnpError::Xml("unbalanced end tag".into()))?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None if root.is_none() => root = Some(node),
                    None => {}
                }
            }
            Event::Text(text) => {
                if let Some(cur) = stack.last_mut() {
                    cur.text.push_str(&text.xml10_content());
                }
            }
            Event::CData(cdata) => {
                if let Some(cur) = stack.last_mut() {
                    cur.text.push_str(&cdata.xml10_content());
                }
            }
            Event::GeneralRef(entity) => {
                if let Some(cur) = stack.last_mut() {
                    match resolve_entity(&entity) {
                        Some(resolved) => cur.text.push_str(&resolved),
                        // Unknown named entity (HTML-ism from some firmwares): keep it literally.
                        None => {
                            cur.text.push('&');
                            cur.text.push_str(&entity);
                            cur.text.push(';');
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if !stack.is_empty() {
        return Err(UpnpError::Xml("unexpected end of document".into()));
    }
    root.ok_or_else(|| UpnpError::Xml("no root element".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_elements_with_local_names() {
        let xml = r#"<?xml version="1.0"?><s:Envelope xmlns:s="x"><s:Body><u:PlayResponse xmlns:u="y"><A>1</A><B/></u:PlayResponse></s:Body></s:Envelope>"#;
        let root = parse(xml).unwrap();
        assert_eq!(root.name, "Envelope");
        let resp = root.path(&["Body", "PlayResponse"]).unwrap();
        assert_eq!(resp.child_text("A").as_deref(), Some("1"));
        assert!(resp.child("B").is_some());
        assert_eq!(resp.child_text("B"), None);
        assert!(root.find("playresponse").is_some());
        assert!(root.find("Missing").is_none());
    }

    #[test]
    fn resolves_entities_cdata_and_attributes() {
        let xml = r#"<root a:x="1 &amp; 2" y='q'>A &amp; B &lt;c&gt; &#65;&#x42; &nbsp;<![CDATA[<raw>]]></root>"#;
        let root = parse(xml).unwrap();
        assert_eq!(root.text, "A & B <c> AB &nbsp;<raw>");
        assert_eq!(root.attr("x"), Some("1 & 2"));
        assert_eq!(root.attr("Y"), Some("q"));
        assert_eq!(root.attr("z"), None);
    }

    #[test]
    fn decodes_double_escaped_payloads_in_two_passes() {
        let xml = "<r><Meta>&lt;DIDL-Lite&gt;&lt;item&gt;&lt;dc:title&gt;Tom &amp;amp; Jerry&lt;/dc:title&gt;&lt;/item&gt;&lt;/DIDL-Lite&gt;</Meta></r>";
        let inner = parse(xml).unwrap().child_text("Meta").unwrap();
        let didl = parse(&inner).unwrap();
        assert_eq!(didl.find("title").unwrap().text, "Tom & Jerry");
    }

    #[test]
    fn tolerates_bom_whitespace_and_comments() {
        let root = parse("\u{feff}\n  <?xml version=\"1.0\"?><!-- c --><r><x>1</x></r>").unwrap();
        assert_eq!(root.child_text("x").as_deref(), Some("1"));
    }

    #[test]
    fn children_named_filters_and_preserves_order() {
        let root = parse("<r><s>1</s><t/><s>2</s></r>").unwrap();
        let texts: Vec<_> = root.children_named("s").map(|n| n.text.clone()).collect();
        assert_eq!(texts, vec!["1", "2"]);
    }

    #[test]
    fn rejects_malformed_or_oversized_input() {
        assert!(parse("").is_err());
        assert!(parse("<a><b></a>").is_err());
        assert!(parse("<a>").is_err());
        assert!(parse(&"x".repeat(MAX_XML_BYTES + 1)).is_err());
        let deep = format!(
            "{}{}",
            "<a>".repeat(MAX_DEPTH + 1),
            "</a>".repeat(MAX_DEPTH + 1)
        );
        assert!(parse(&deep).is_err());
    }

    #[test]
    fn empty_root_element_is_accepted() {
        let root = parse(r#"<root k="v"/>"#).unwrap();
        assert_eq!(root.name, "root");
        assert_eq!(root.attr("k"), Some("v"));
    }

    #[test]
    fn resolve_entity_handles_numeric_forms() {
        assert_eq!(resolve_entity("#x41").as_deref(), Some("A"));
        assert_eq!(resolve_entity("#X41").as_deref(), Some("A"));
        assert_eq!(resolve_entity("#66").as_deref(), Some("B"));
        assert_eq!(resolve_entity("#xZZ"), None);
        assert_eq!(resolve_entity("#xD800"), None);
        assert_eq!(resolve_entity("copy"), None);
    }
}

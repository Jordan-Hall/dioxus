use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::ToTokens;
use syn::{Error, LitStr};

use crate::rsx;

#[derive(Clone, Debug)]
enum Node {
    Text(String),
    Element(Element),
    If { branches: Vec<(String, Vec<Node>)>, otherwise: Option<Vec<Node>> },
    For { pattern: String, iterable: String, track: Option<String>, body: Vec<Node>, empty: Option<Vec<Node>> },
    Switch { expression: String, cases: Vec<(Option<String>, Vec<Node>)> },
    Let { name: String, expression: String },
    Defer { body: Vec<Node>, placeholder: Option<Vec<Node>>, loading: Option<Vec<Node>>, error: Option<Vec<Node>> },
    Boundary { body: Vec<Node>, error: Option<Vec<Node>> },
}

#[derive(Clone, Debug)]
struct Element {
    name: String,
    attrs: Vec<Attribute>,
    children: Vec<Node>,
}

#[derive(Clone, Debug)]
struct Attribute {
    name: String,
    value: Option<String>,
    kind: AttributeKind,
}

#[derive(Clone, Debug)]
enum AttributeKind {
    Static,
    Property(String),
    Event(String),
    TwoWay(String),
    Reference(String),
    Generated(String),
}

struct Parser<'a> {
    source: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, pos: 0 }
    }

    fn parse(mut self) -> Result<Vec<Node>, String> {
        let nodes = self.parse_nodes(None, false, None)?;
        self.skip_ws();
        if self.pos != self.source.len() {
            return self.error("unexpected trailing template input");
        }
        Ok(nodes)
    }

    fn parse_nodes(
        &mut self,
        stop_tag: Option<&str>,
        stop_brace: bool,
        stop_control: Option<&str>,
    ) -> Result<Vec<Node>, String> {
        let mut nodes = Vec::new();
        while self.pos < self.source.len() {
            if self.starts_with("<!--") {
                self.skip_comment()?;
                continue;
            }
            if let Some(control) = stop_control {
                if self.at_control(control) {
                    return Ok(nodes);
                }
            }
            if stop_brace && self.starts_with("}") {
                self.pos += 1;
                return Ok(nodes);
            }
            if self.starts_with("</") {
                let closing = self.parse_closing_tag()?;
                match stop_tag {
                    Some(expected) if expected.eq_ignore_ascii_case(&closing) => return Ok(nodes),
                    Some(expected) => return self.error(&format!("closing tag </{closing}> does not match <{expected}>")),
                    None => return self.error(&format!("unexpected closing tag </{closing}>")),
                }
            }
            if self.is_tag_start() {
                nodes.push(Node::Element(self.parse_element()?));
                continue;
            }
            if self.starts_with("@") {
                if let Some(node) = self.try_parse_control()? {
                    nodes.push(node);
                    continue;
                }
            }
            let text = self.parse_text(stop_brace);
            if !text.is_empty() {
                nodes.push(Node::Text(text));
            } else if self.pos < self.source.len() {
                return self.error("unable to parse template text");
            }
        }
        if let Some(tag) = stop_tag {
            return self.error(&format!("missing closing tag </{tag}>"));
        }
        if stop_brace {
            return self.error("missing closing brace for Angular control-flow block");
        }
        Ok(nodes)
    }

    fn try_parse_control(&mut self) -> Result<Option<Node>, String> {
        if self.at_control("if") { return self.parse_if().map(Some); }
        if self.at_control("for") { return self.parse_for().map(Some); }
        if self.at_control("switch") { return self.parse_switch().map(Some); }
        if self.at_control("let") { return self.parse_let().map(Some); }
        if self.at_control("defer") { return self.parse_defer().map(Some); }
        if self.at_control("boundary") { return self.parse_boundary().map(Some); }
        Ok(None)
    }

    fn parse_if(&mut self) -> Result<Node, String> {
        self.consume_control("if")?;
        let header = self.read_parenthesized()?;
        let (condition, suffix) = split_once_top_level(&header, ';')
            .map(|(condition, rest)| (condition.trim().to_owned(), Some(rest.trim().to_owned())))
            .unwrap_or((header.trim().to_owned(), None));
        if let Some(suffix) = suffix {
            if !suffix.is_empty() {
                return self.error("Angular @if aliases require a Rust binding and are not yet supported; move the binding to @let");
            }
        }
        if condition.is_empty() {
            return self.error("@if requires a condition");
        }

        let mut branches = vec![(condition, self.parse_block()?)];
        let mut otherwise = None;
        loop {
            let previous = self.pos;
            self.skip_ws();
            if self.at_control("else") {
                self.consume_control("else")?;
                self.skip_ws();
                if self.at_control("if") {
                    self.consume_control("if")?;
                    let condition = self.read_parenthesized()?.trim().to_owned();
                    branches.push((condition, self.parse_block()?));
                } else {
                    otherwise = Some(self.parse_block()?);
                    break;
                }
            } else {
                self.pos = previous;
                break;
            }
        }
        Ok(Node::If { branches, otherwise })
    }

    fn parse_for(&mut self) -> Result<Node, String> {
        self.consume_control("for")?;
        let header = self.read_parenthesized()?;
        let mut parts = split_top_level(&header, ';');
        let main = parts.remove(0).trim().to_owned();
        let Some((pattern, iterable)) = split_once_word(&main, "of") else {
            return self.error("Angular @for syntax is: @for (item of items; track item.id) { ... }");
        };
        let pattern = pattern.trim().to_owned();
        let iterable = iterable.trim().to_owned();
        if pattern.is_empty() || iterable.is_empty() {
            return self.error("@for requires both an item pattern and an iterable");
        }
        let mut track = None;
        for part in parts {
            let part = part.trim();
            if let Some(expr) = part.strip_prefix("track ") {
                if track.is_some() {
                    return self.error("@for may contain only one track clause");
                }
                if expr.trim().is_empty() {
                    return self.error("@for track clause requires an expression");
                }
                track = Some(expr.trim().to_owned());
            } else if !part.is_empty() {
                return self.error(&format!("unsupported @for clause: {part}"));
            }
        }
        let mut body = self.parse_block()?;
        if let Some(track_expr) = track.as_deref() {
            inject_track_keys(&mut body, track_expr);
        }
        let previous = self.pos;
        self.skip_ws();
        let empty = if self.at_control("empty") {
            self.consume_control("empty")?;
            Some(self.parse_block()?)
        } else {
            self.pos = previous;
            None
        };
        Ok(Node::For { pattern, iterable, track, body, empty })
    }

    fn parse_switch(&mut self) -> Result<Node, String> {
        self.consume_control("switch")?;
        let expression = self.read_parenthesized()?.trim().to_owned();
        if expression.is_empty() {
            return self.error("@switch requires an expression");
        }
        self.skip_ws();
        self.expect_char('{')?;
        let mut cases = Vec::new();
        let mut has_default = false;
        loop {
            self.skip_ws();
            if self.pos >= self.source.len() {
                return self.error("missing closing brace for @switch");
            }
            if self.starts_with("}") {
                self.pos += 1;
                break;
            }
            if self.at_control("case") {
                if has_default {
                    return self.error("@case cannot appear after @default");
                }
                self.consume_control("case")?;
                let value = self.read_parenthesized()?.trim().to_owned();
                if value.is_empty() {
                    return self.error("@case requires an expression");
                }
                cases.push((Some(value), self.parse_block()?));
                continue;
            }
            if self.at_control("default") {
                if has_default {
                    return self.error("@switch may contain only one @default");
                }
                self.consume_control("default")?;
                has_default = true;
                cases.push((None, self.parse_block()?));
                continue;
            }
            return self.error("@switch can contain only @case and @default blocks");
        }
        if cases.is_empty() {
            return self.error("@switch requires at least one @case or @default");
        }
        Ok(Node::Switch { expression, cases })
    }

    fn parse_let(&mut self) -> Result<Node, String> {
        self.consume_control("let")?;
        let statement = self.read_statement()?;
        let Some((name, expression)) = split_once_top_level(&statement, '=') else {
            return self.error("Angular @let syntax is: @let name = expression;");
        };
        let name = name.trim().to_owned();
        let expression = expression.trim().to_owned();
        if !is_rust_ident(&name) {
            return self.error("@let name must be a Rust identifier");
        }
        if expression.is_empty() {
            return self.error("@let requires an expression");
        }
        Ok(Node::Let { name, expression })
    }

    fn parse_defer(&mut self) -> Result<Node, String> {
        self.consume_control("defer")?;
        let _triggers = if self.peek_char() == Some('(') { Some(self.read_parenthesized()?) } else { None };
        let body = self.parse_block()?;
        let mut placeholder = None;
        let mut loading = None;
        let mut error = None;
        loop {
            let previous = self.pos;
            self.skip_ws();
            if self.at_control("placeholder") {
                self.consume_control("placeholder")?;
                self.skip_optional_parenthesized()?;
                if placeholder.replace(self.parse_block()?).is_some() {
                    return self.error("@defer may contain only one @placeholder block");
                }
            } else if self.at_control("loading") {
                self.consume_control("loading")?;
                self.skip_optional_parenthesized()?;
                if loading.replace(self.parse_block()?).is_some() {
                    return self.error("@defer may contain only one @loading block");
                }
            } else if self.at_control("error") {
                self.consume_control("error")?;
                if error.replace(self.parse_block()?).is_some() {
                    return self.error("@defer may contain only one @error block");
                }
            } else {
                self.pos = previous;
                break;
            }
        }
        Ok(Node::Defer { body, placeholder, loading, error })
    }

    fn parse_boundary(&mut self) -> Result<Node, String> {
        self.consume_control("boundary")?;
        self.skip_ws();
        self.expect_char('{')?;
        let body = self.parse_nodes(None, true, Some("error"))?;
        let mut error = None;
        if self.at_control("error") {
            self.consume_control("error")?;
            error = Some(self.parse_block()?);
            self.skip_ws();
            self.expect_char('}')?;
        }
        Ok(Node::Boundary { body, error })
    }

    fn parse_block(&mut self) -> Result<Vec<Node>, String> {
        self.skip_ws();
        self.expect_char('{')?;
        self.parse_nodes(None, true, None)
    }

    fn skip_optional_parenthesized(&mut self) -> Result<(), String> {
        self.skip_ws();
        if self.peek_char() == Some('(') {
            self.read_parenthesized()?;
        }
        Ok(())
    }

    fn parse_element(&mut self) -> Result<Element, String> {
        self.expect_char('<')?;
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() || ch == '/' || ch == '>' { break; }
            self.bump_char();
        }
        if start == self.pos {
            return self.error("expected an element name after <");
        }
        let name = self.source[start..self.pos].to_owned();
        let mut attrs = Vec::new();
        let mut self_closing = false;
        loop {
            self.skip_ws();
            if self.starts_with("/>") {
                self.pos += 2;
                self_closing = true;
                break;
            }
            if self.starts_with(">") {
                self.pos += 1;
                break;
            }
            if self.pos >= self.source.len() {
                return self.error("unterminated opening tag");
            }
            let attr_start = self.pos;
            while let Some(ch) = self.peek_char() {
                if ch.is_whitespace() || ch == '=' || ch == '>' || ch == '/' { break; }
                self.bump_char();
            }
            if attr_start == self.pos {
                return self.error("invalid attribute syntax");
            }
            let raw_name = self.source[attr_start..self.pos].to_owned();
            self.skip_ws();
            let value = if self.consume_if("=") {
                self.skip_ws();
                Some(self.read_attribute_value()?)
            } else { None };
            attrs.push(parse_attribute(raw_name, value));
        }
        let children = if self_closing || is_void_element(&name) {
            Vec::new()
        } else {
            self.parse_nodes(Some(&name), false, None)?
        };
        Ok(Element { name, attrs, children })
    }

    fn parse_closing_tag(&mut self) -> Result<String, String> {
        self.pos += 2;
        self.skip_ws();
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() || ch == '>' { break; }
            self.bump_char();
        }
        if start == self.pos { return self.error("expected closing tag name"); }
        let name = self.source[start..self.pos].to_owned();
        self.skip_ws();
        self.expect_char('>')?;
        Ok(name)
    }

    fn read_attribute_value(&mut self) -> Result<String, String> {
        let Some(first) = self.peek_char() else { return self.error("expected attribute value"); };
        if first == '"' || first == '\'' {
            self.bump_char();
            let start = self.pos;
            while let Some(ch) = self.peek_char() {
                if ch == first {
                    let value = self.source[start..self.pos].to_owned();
                    self.bump_char();
                    return Ok(decode_html_entities(&value));
                }
                self.bump_char();
            }
            return self.error("unterminated quoted attribute value");
        }
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() || ch == '>' { break; }
            self.bump_char();
        }
        Ok(decode_html_entities(&self.source[start..self.pos]))
    }

    fn parse_text(&mut self, stop_brace: bool) -> String {
        let start = self.pos;
        while self.pos < self.source.len() {
            if self.starts_with("{{") {
                if let Some(end) = self.source[self.pos + 2..].find("}}") {
                    self.pos += 2 + end + 2;
                    continue;
                }
            }
            if self.is_tag_start() || self.starts_with("</")
                || (self.source[self.pos..].starts_with('@') && self.is_known_control_start())
                || (stop_brace && self.starts_with("}"))
            {
                break;
            }
            self.bump_char();
        }
        self.source[start..self.pos].to_owned()
    }

    fn read_parenthesized(&mut self) -> Result<String, String> {
        self.skip_ws();
        self.expect_char('(')?;
        self.read_balanced('(', ')')
    }

    fn read_statement(&mut self) -> Result<String, String> {
        let start = self.pos;
        let mut depth = 0_i32;
        let mut quote = None;
        let mut escaped = false;
        while let Some(ch) = self.peek_char() {
            self.bump_char();
            if let Some(q) = quote {
                if escaped { escaped = false; }
                else if ch == '\\' { escaped = true; }
                else if ch == q { quote = None; }
                continue;
            }
            match ch {
                '"' | '\'' => quote = Some(ch),
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                ';' if depth <= 0 => return Ok(self.source[start..self.pos - 1].to_owned()),
                _ => {}
            }
        }
        self.error("@let statement must end with semicolon")
    }

    fn read_balanced(&mut self, open: char, close: char) -> Result<String, String> {
        let start = self.pos;
        let mut depth = 1_i32;
        let mut quote = None;
        let mut escaped = false;
        while let Some(ch) = self.peek_char() {
            self.bump_char();
            if let Some(q) = quote {
                if escaped { escaped = false; }
                else if ch == '\\' { escaped = true; }
                else if ch == q { quote = None; }
                continue;
            }
            match ch {
                '"' | '\'' => quote = Some(ch),
                c if c == open => depth += 1,
                c if c == close => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(self.source[start..self.pos - close.len_utf8()].to_owned());
                    }
                }
                _ => {}
            }
        }
        self.error(&format!("unclosed {open} delimiter"))
    }

    fn skip_comment(&mut self) -> Result<(), String> {
        let Some(end) = self.source[self.pos + 4..].find("-->") else {
            return self.error("unterminated HTML comment");
        };
        self.pos += 4 + end + 3;
        Ok(())
    }

    fn is_tag_start(&self) -> bool {
        if !self.starts_with("<") { return false; }
        let next = self.source[self.pos + 1..].chars().next();
        matches!(next, Some(ch) if ch.is_ascii_alphabetic() || ch == '!')
    }

    fn is_known_control_start(&self) -> bool {
        ["if", "for", "switch", "let", "defer", "boundary", "else", "empty", "case", "default", "placeholder", "loading", "error"]
            .iter()
            .any(|name| self.at_control(name))
    }

    fn at_control(&self, name: &str) -> bool {
        let prefix = format!("@{name}");
        if !self.source[self.pos..].starts_with(&prefix) { return false; }
        self.source[self.pos + prefix.len()..].chars().next()
            .map_or(true, |ch| !ch.is_ascii_alphanumeric() && ch != '_')
    }

    fn consume_control(&mut self, name: &str) -> Result<(), String> {
        if !self.at_control(name) { return self.error(&format!("expected @{name}")); }
        self.pos += name.len() + 1;
        Ok(())
    }

    fn skip_ws(&mut self) {
        while self.peek_char().is_some_and(char::is_whitespace) { self.bump_char(); }
    }

    fn expect_char(&mut self, expected: char) -> Result<(), String> {
        self.skip_ws();
        if self.peek_char() == Some(expected) {
            self.bump_char();
            Ok(())
        } else {
            self.error(&format!("expected {expected}"))
        }
    }

    fn consume_if(&mut self, expected: &str) -> bool {
        if self.starts_with(expected) { self.pos += expected.len(); true } else { false }
    }

    fn starts_with(&self, needle: &str) -> bool { self.source[self.pos..].starts_with(needle) }
    fn peek_char(&self) -> Option<char> { self.source[self.pos..].chars().next() }
    fn bump_char(&mut self) { if let Some(ch) = self.peek_char() { self.pos += ch.len_utf8(); } }
    fn error<T>(&self, message: &str) -> Result<T, String> { Err(format!("{message} at byte {}", self.pos)) }
}

fn parse_attribute(raw_name: String, value: Option<String>) -> Attribute {
    if raw_name.starts_with("[(") && raw_name.ends_with(")]") {
        let name = raw_name[2..raw_name.len() - 2].to_owned();
        return Attribute { name: name.clone(), value, kind: AttributeKind::TwoWay(name) };
    }
    if raw_name.starts_with('[') && raw_name.ends_with(']') {
        let name = raw_name[1..raw_name.len() - 1].to_owned();
        return Attribute { name: name.clone(), value, kind: AttributeKind::Property(name) };
    }
    if raw_name.starts_with('(') && raw_name.ends_with(')') {
        let name = raw_name[1..raw_name.len() - 1].to_owned();
        return Attribute { name: name.clone(), value, kind: AttributeKind::Event(name) };
    }
    if let Some(name) = raw_name.strip_prefix("bindon-") {
        return Attribute { name: name.to_owned(), value, kind: AttributeKind::TwoWay(name.to_owned()) };
    }
    if let Some(name) = raw_name.strip_prefix("bind-") {
        return Attribute { name: name.to_owned(), value, kind: AttributeKind::Property(name.to_owned()) };
    }
    if let Some(name) = raw_name.strip_prefix("on-") {
        return Attribute { name: name.to_owned(), value, kind: AttributeKind::Event(name.to_owned()) };
    }
    if raw_name.starts_with('#') || raw_name.starts_with("ref-") {
        let name = raw_name.strip_prefix('#').or_else(|| raw_name.strip_prefix("ref-")).unwrap_or(&raw_name);
        return Attribute { name: name.to_owned(), value, kind: AttributeKind::Reference(name.to_owned()) };
    }
    Attribute { name: raw_name, value, kind: AttributeKind::Static }
}

fn render_nodes(nodes: &[Node]) -> Result<String, String> {
    let mut out = String::new();
    let mut index = 0;
    while index < nodes.len() {
        match &nodes[index] {
            Node::Let { name, expression } => {
                let remaining = render_nodes(&nodes[index + 1..])?;
                out.push_str(&format!("{{ let {name} = {expression}; ::dioxus::prelude::rsx! {{ {remaining} }} }}"));
                break;
            }
            node => {
                out.push_str(&render_node(node)?);
                index += 1;
                if index < nodes.len() { out.push_str(", "); }
            }
        }
    }
    Ok(out)
}

fn render_node(node: &Node) -> Result<String, String> {
    match node {
        Node::Text(text) => Ok(render_interpolated_string(&decode_html_entities(text))),
        Node::Element(el) => render_element(el),
        Node::If { branches, otherwise } => {
            let mut out = String::new();
            for (index, (condition, body)) in branches.iter().enumerate() {
                if index > 0 { out.push_str(" else "); }
                out.push_str(&format!("if {condition} {{ {} }}", render_nodes(body)?));
            }
            if let Some(body) = otherwise { out.push_str(&format!(" else {{ {} }}", render_nodes(body)?)); }
            Ok(out)
        }
        Node::For { pattern, iterable, body, empty: None, .. } => {
            Ok(format!("for {pattern} in {iterable} {{ {} }}", render_nodes(body)?))
        }
        Node::For { pattern, iterable, body, empty: Some(empty), .. } => {
            let body = render_nodes(body)?;
            let empty = render_nodes(empty)?;
            Ok(format!(
                "{{ let __angular_items: ::std::vec::Vec<_> = ({iterable}).into_iter().collect(); if __angular_items.is_empty() {{ ::dioxus::prelude::rsx! {{ {empty} }} }} else {{ ::dioxus::prelude::rsx! {{ for {pattern} in __angular_items {{ {body} }} }} }} }}"
            ))
        }
        Node::Switch { expression, cases } => {
            let mut out = String::new();
            for (index, (case, body)) in cases.iter().enumerate() {
                if index > 0 { out.push_str(" else "); }
                match case {
                    Some(case) => out.push_str(&format!("if __angular_switch_value == ({case}) {{ {} }}", render_nodes(body)?)),
                    None => out.push_str(&format!("if true {{ {} }}", render_nodes(body)?)),
                }
            }
            Ok(format!("{{ let __angular_switch_value = ({expression}); ::dioxus::prelude::rsx! {{ {out} }} }}"))
        }
        Node::Defer { body, placeholder, loading, error } => {
            let fallback = placeholder.as_ref().or(loading.as_ref()).map(|nodes| render_nodes(nodes)).transpose()?.unwrap_or_default();
            let body = render_nodes(body)?;
            if let Some(error_nodes) = error {
                Ok(format!(
                    "ErrorBoundary {{ handle_error: move |_| ::dioxus::prelude::rsx! {{ {} }}, SuspenseBoundary {{ fallback: move |_| ::dioxus::prelude::rsx! {{ {fallback} }}, {body} }} }}",
                    render_nodes(error_nodes)?
                ))
            } else {
                Ok(format!("SuspenseBoundary {{ fallback: move |_| ::dioxus::prelude::rsx! {{ {fallback} }}, {body} }}"))
            }
        }
        Node::Boundary { body, error } => {
            if let Some(error) = error {
                Ok(format!("ErrorBoundary {{ handle_error: move |_| ::dioxus::prelude::rsx! {{ {} }}, {} }}", render_nodes(error)?, render_nodes(body)?))
            } else {
                Ok(format!("ErrorBoundary {{ {} }}", render_nodes(body)?))
            }
        }
        Node::Let { .. } => unreachable!(),
    }
}

fn render_element(el: &Element) -> Result<String, String> {
    let mut fields = Vec::new();
    for attr in &el.attrs {
        match &attr.kind {
            AttributeKind::Static => {
                let name = render_attribute_name(&attr.name);
                let value = match attr.value.as_deref() {
                    Some(value) => render_interpolated_string(value),
                    None if is_boolean_attribute(&attr.name) => "true".to_owned(),
                    None => "true".to_owned(),
                };
                fields.push(format!("{name}: {value}"));
            }
            AttributeKind::Property(name) => {
                let value = required_value(attr)?;
                if let Some(property) = name.strip_prefix("attr.") {
                    fields.push(format!("{}: {{ {value} }}", render_attribute_name(property)));
                } else {
                    fields.push(format!("{}: {{ {value} }}", render_attribute_name(name)));
                }
            }
            AttributeKind::Event(event) => {
                let value = required_value(attr)?.replace("$event", "__angular_event");
                fields.push(format!(
                    "{}: move |__angular_event| {{ {value}; }}",
                    render_attribute_name(&format!("on{}", normalize_event_name(event)))
                ));
            }
            AttributeKind::TwoWay(name) => {
                let value = required_value(attr)?;
                fields.push(format!("{}: {{ {value} }}", render_attribute_name(name)));
                let (event, getter) = match name.as_str() {
                    "value" => ("input", "value()"),
                    "checked" => ("change", "checked()"),
                    _ => ("change", "value()"),
                };
                fields.push(format!(
                    "{}: move |__angular_event| {{ ({value}).set(__angular_event.{}); }}",
                    render_attribute_name(&format!("on{event}")),
                    getter
                ));
            }
            AttributeKind::Reference(name) => fields.push(format!("node_ref: {{ {name} }}")),
            AttributeKind::Generated(value) => fields.push(format!("{}: {value}", render_attribute_name(&attr.name))),
        }
    }
    let children = render_nodes(&el.children)?;
    if !children.is_empty() { fields.push(children); }
    Ok(format!("{} {{ {} }}", el.name, fields.join(", ")))
}

fn required_value(attr: &Attribute) -> Result<String, String> {
    attr.value.clone().filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("Angular binding {} requires a value", attr.name))
}

fn render_attribute_name(name: &str) -> String {
    if is_rust_ident(name) || name.contains(':') { name.to_owned() }
    else { syn::LitStr::new(name, Span::call_site()).to_token_stream().to_string() }
}

fn render_interpolated_string(source: &str) -> String {
    let mut formatted = String::new();
    let mut pos = 0;
    while pos < source.len() {
        if source[pos..].starts_with("{{") {
            if let Some(relative_end) = source[pos + 2..].find("}}") {
                let end = pos + 2 + relative_end;
                let expression = source[pos + 2..end].trim();
                if expression.is_empty() {
                    formatted.push_str("{{}}");
                } else {
                    formatted.push('{');
                    formatted.push_str(expression);
                    formatted.push('}');
                }
                pos = end + 2;
                continue;
            }
        }
        let ch = source[pos..].chars().next().unwrap();
        match ch {
            '{' => formatted.push_str("{{"),
            '}' => formatted.push_str("}}"),
            _ => formatted.push(ch),
        }
        pos += ch.len_utf8();
    }
    syn::LitStr::new(&formatted, Span::call_site()).to_token_stream().to_string()
}

fn inject_track_keys(nodes: &mut [Node], track: &str) {
    for node in nodes {
        match node {
            Node::Element(el) => el.attrs.insert(0, Attribute {
                name: "key".to_owned(),
                value: None,
                kind: AttributeKind::Generated(format!("\"{{{track}}}\"")),
            }),
            Node::If { branches, otherwise } => {
                for (_, body) in branches { inject_track_keys(body, track); }
                if let Some(body) = otherwise { inject_track_keys(body, track); }
            }
            _ => {}
        }
    }
}

fn normalize_event_name(event: &str) -> String { event.trim().replace('-', "_") }

fn is_boolean_attribute(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(),
        "allowfullscreen" | "async" | "autofocus" | "autoplay" | "checked" | "controls" |
        "default" | "defer" | "disabled" | "formnovalidate" | "hidden" | "inert" |
        "ismap" | "itemscope" | "loop" | "multiple" | "muted" | "nomodule" | "novalidate" |
        "open" | "playsinline" | "readonly" | "required" | "reversed" | "selected")
}

fn is_void_element(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(),
        "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link" |
        "meta" | "param" | "source" | "track" | "wbr")
}

fn is_rust_ident(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(ch) if ch == '_' || ch.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn split_once_word<'a>(source: &'a str, word: &str) -> Option<(&'a str, &'a str)> {
    let mut depth = 0_i32;
    let mut quote = None;
    let bytes = source.as_bytes();
    let mut i = 0;
    while i + word.len() <= bytes.len() {
        let ch = bytes[i] as char;
        if let Some(q) = quote {
            if ch == q && (i == 0 || bytes[i - 1] != b'\\') { quote = None; }
            i += 1;
            continue;
        }
        if ch == '"' || ch == '\'' { quote = Some(ch); i += 1; continue; }
        if ch == '(' || ch == '[' || ch == '{' { depth += 1; }
        else if ch == ')' || ch == ']' || ch == '}' { depth -= 1; }
        else if depth == 0 && source[i..].starts_with(word) {
            let before_ok = i == 0 || source[..i].chars().next_back().is_some_and(char::is_whitespace);
            let after = i + word.len();
            let after_ok = after == source.len() || source[after..].chars().next().is_some_and(char::is_whitespace);
            if before_ok && after_ok { return Some((&source[..i], &source[after..])); }
        }
        i += ch.len_utf8();
    }
    None
}

fn split_once_top_level(source: &str, separator: char) -> Option<(&str, &str)> {
    let mut depth = 0_i32;
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in source.char_indices() {
        if let Some(q) = quote {
            if escaped { escaped = false; }
            else if ch == '\\' { escaped = true; }
            else if ch == q { quote = None; }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            c if c == separator && depth == 0 => return Some((&source[..index], &source[index + ch.len_utf8()..])),
            _ => {}
        }
    }
    None
}

fn split_top_level(source: &str, separator: char) -> Vec<String> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0_i32;
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in source.char_indices() {
        if let Some(q) = quote {
            if escaped { escaped = false; }
            else if ch == '\\' { escaped = true; }
            else if ch == q { quote = None; }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            c if c == separator && depth == 0 => {
                result.push(source[start..index].to_owned());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    result.push(source[start..].to_owned());
    result
}

fn decode_html_entities(source: &str) -> String {
    let mut out = String::new();
    let mut pos = 0;
    while pos < source.len() {
        if source.as_bytes()[pos] == b'&' {
            if let Some((entity, value)) = decode_entity_at_start(&source[pos..]) {
                out.push_str(&value);
                pos += entity.len();
                continue;
            }
        }
        let ch = source[pos..].chars().next().unwrap();
        out.push(ch);
        pos += ch.len_utf8();
    }
    out
}

fn decode_entity_at_start(source: &str) -> Option<(&str, String)> {
    let end = source.find(';')?;
    if end > 12 { return None; }
    let entity = &source[..=end];
    let name = &entity[1..end];
    let value = match name {
        "amp" => "&".to_owned(),
        "lt" => "<".to_owned(),
        "gt" => ">".to_owned(),
        "quot" => "\"".to_owned(),
        "apos" => "'".to_owned(),
        "nbsp" => "\u{00a0}".to_owned(),
        _ if name.starts_with("#x") || name.starts_with("#X") => char::from_u32(u32::from_str_radix(&name[2..], 16).ok()?)?.to_string(),
        _ if name.starts_with('#') => char::from_u32(name[1..].parse().ok()?)?.to_string(),
        _ => return None,
    };
    Some((entity, value))
}

pub(crate) fn expand(tokens: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(tokens as LitStr);
    let source = input.value();
    let parsed = match Parser::new(&source).parse() {
        Ok(parsed) => parsed,
        Err(message) => return Error::new(input.span(), message).to_compile_error().into(),
    };
    let rsx_source = match render_nodes(&parsed) {
        Ok(source) => source,
        Err(message) => return Error::new(input.span(), message).to_compile_error().into(),
    };
    match syn::parse_str::<rsx::CallBody>(&rsx_source) {
        Ok(body) => body.into_token_stream().into(),
        Err(error) => Error::new(input.span(), format!("failed to lower Angular template to Dioxus: {error}\nGenerated RSX: {rsx_source}"))
            .to_compile_error().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lower(source: &str) -> String {
        let nodes = Parser::new(source).parse().unwrap();
        let generated = render_nodes(&nodes).unwrap();
        syn::parse_str::<rsx::CallBody>(&generated)
            .unwrap_or_else(|error| panic!("generated invalid RSX: {error}\\n{generated}"));
        generated
    }

    #[test]
    fn parses_html_elements_and_text() {
        let rsx = lower(r#"<main class="app"><h1>Hello</h1><input disabled /></main>"#);
        assert!(rsx.contains("main"));
        assert!(rsx.contains("h1"));
        assert!(rsx.contains("input"));
        assert!(rsx.contains("disabled: true"));
    }

    #[test]
    fn parses_interpolation_and_property_binding() {
        let rsx = lower(r#"<button [disabled]="is_disabled">Hello {{ name }}</button>"#);
        assert!(rsx.contains("disabled: { is_disabled }"));
        assert!(rsx.contains("Hello {name}"));
    }

    #[test]
    fn parses_events_and_two_way_binding() {
        let rsx = lower(r#"<button (click)="save($event)">Save</button><input [(value)]="name" />"#);
        assert!(rsx.contains("onclick"));
        assert!(rsx.contains("oninput"));
        assert!(rsx.contains("save(__angular_event)"));
        assert!(rsx.contains("name).set"));
    }

    #[test]
    fn parses_if_else_and_for_empty() {
        let rsx = lower(r#"@if (visible) { <p>Visible</p> } @else { <p>Hidden</p> }
            @for (item of items; track item.id) { <p>{{ item.name }}</p> } @empty { <p>Empty</p> }"#);
        assert!(rsx.contains("if visible"));
        assert!(rsx.contains("for item in __angular_items"));
        assert!(rsx.contains("__angular_items.is_empty()"));
        assert!(rsx.contains("key"));
    }

    #[test]
    fn parses_switch_and_let() {
        let rsx = lower(r#"@let count = total; @switch (count) { @case (0) { <p>Zero</p> } @default { <p>Many</p> } }"#);
        assert!(rsx.contains("let count = total"));
        assert!(rsx.contains("__angular_switch_value"));
        assert!(rsx.contains("if true"));
    }

    #[test]
    fn reports_mismatched_tags() {
        let err = Parser::new("<div></span>").parse().unwrap_err();
        assert!(err.contains("does not match"));
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(decode_html_entities("a &amp; b &lt; c"), "a & b < c");
    }
}

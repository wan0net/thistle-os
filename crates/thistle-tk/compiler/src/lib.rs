// SPDX-License-Identifier: BSD-3-Clause
//! Host-only declarative UI compiler for `thistle-tk`.
//!
//! This crate is intentionally `std` and must only run during builds. The
//! generated Rust constructs ordinary `thistle_tk` widget trees, so parser
//! dependencies never ship to ESP32 devices.

use std::collections::BTreeMap;
use std::env;
use std::fmt::{self, Write};
use std::fs;
use std::path::{Path, PathBuf};

/// Compiler error with a human-readable message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    message: String,
}

impl Error {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Return the diagnostic text.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

/// Options for Rust code generation.
#[derive(Debug, Clone)]
pub struct CompileOptions {
    /// Name of the generated UI handle struct.
    pub struct_name: String,
    /// Name of the generated build function.
    pub fn_name: String,
}

impl CompileOptions {
    pub fn new(struct_name: impl Into<String>, fn_name: impl Into<String>) -> Self {
        Self {
            struct_name: struct_name.into(),
            fn_name: fn_name.into(),
        }
    }
}

/// Compile XML-like markup and simple CSS into Rust source.
pub fn compile_to_rust(markup: &str, css: &str, options: &CompileOptions) -> Result<String> {
    validate_rust_ident(&options.struct_name, "struct name")?;
    validate_rust_ident(&options.fn_name, "function name")?;

    let mut root = parse_markup(markup)?;
    let rules = parse_css(css)?;
    apply_styles(&mut root, &rules);
    Codegen::new(options).generate(&root)
}

/// Compile XML-like markup and simple CSS files into Rust source.
pub fn compile_files_to_rust(
    markup_path: impl AsRef<Path>,
    css_path: impl AsRef<Path>,
    options: &CompileOptions,
) -> Result<String> {
    let markup_path = markup_path.as_ref();
    let css_path = css_path.as_ref();
    let markup = fs::read_to_string(markup_path).map_err(|err| {
        Error::new(format!(
            "failed to read UI markup `{}`: {err}",
            markup_path.display()
        ))
    })?;
    let css = fs::read_to_string(css_path).map_err(|err| {
        Error::new(format!(
            "failed to read UI CSS `{}`: {err}",
            css_path.display()
        ))
    })?;
    compile_to_rust(&markup, &css, options)
}

/// Compile XML-like markup and simple CSS files, then write generated Rust.
pub fn compile_files_to_path(
    markup_path: impl AsRef<Path>,
    css_path: impl AsRef<Path>,
    out_path: impl AsRef<Path>,
    options: &CompileOptions,
) -> Result<()> {
    let out_path = out_path.as_ref();
    let source = compile_files_to_rust(markup_path, css_path, options)?;
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| {
                Error::new(format!(
                    "failed to create output directory `{}`: {err}",
                    parent.display()
                ))
            })?;
        }
    }
    fs::write(out_path, source).map_err(|err| {
        Error::new(format!(
            "failed to write generated Rust `{}`: {err}",
            out_path.display()
        ))
    })
}

/// Compile UI files from a Cargo build script into `OUT_DIR`.
///
/// This prints `cargo:rerun-if-changed` directives for both inputs and returns
/// the generated Rust path so the caller can mirror it in custom diagnostics.
pub fn compile_for_build_script(
    markup_path: impl AsRef<Path>,
    css_path: impl AsRef<Path>,
    out_file_name: impl AsRef<Path>,
    options: &CompileOptions,
) -> Result<PathBuf> {
    let markup_path = markup_path.as_ref();
    let css_path = css_path.as_ref();
    let out_file_name = out_file_name.as_ref();
    println!("cargo:rerun-if-changed={}", markup_path.display());
    println!("cargo:rerun-if-changed={}", css_path.display());

    let out_dir = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| Error::new("OUT_DIR is not set; compile_for_build_script must run from build.rs"))?;
    let out_path = out_dir.join(out_file_name);
    compile_files_to_path(markup_path, css_path, &out_path, options)?;
    Ok(out_path)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ElementKind {
    Screen,
    Row,
    Column,
    Label,
    Button,
    Divider,
    Spacer,
    TextInput,
    ListItem,
    Progress,
}

impl ElementKind {
    fn parse(tag: &str) -> Option<Self> {
        match tag {
            "screen" => Some(Self::Screen),
            "row" => Some(Self::Row),
            "column" => Some(Self::Column),
            "label" => Some(Self::Label),
            "button" => Some(Self::Button),
            "divider" => Some(Self::Divider),
            "spacer" => Some(Self::Spacer),
            "text-input" => Some(Self::TextInput),
            "list-item" => Some(Self::ListItem),
            "progress" => Some(Self::Progress),
            _ => None,
        }
    }

    fn tag(self) -> &'static str {
        match self {
            Self::Screen => "screen",
            Self::Row => "row",
            Self::Column => "column",
            Self::Label => "label",
            Self::Button => "button",
            Self::Divider => "divider",
            Self::Spacer => "spacer",
            Self::TextInput => "text-input",
            Self::ListItem => "list-item",
            Self::Progress => "progress",
        }
    }

    fn default_direction(self) -> Option<Direction> {
        match self {
            Self::Screen | Self::Column => Some(Direction::Column),
            Self::Row => Some(Direction::Row),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct Node {
    kind: ElementKind,
    id: Option<String>,
    classes: Vec<String>,
    attrs: BTreeMap<String, String>,
    children: Vec<Node>,
    style: Style,
}

impl Node {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }
}

fn parse_markup(markup: &str) -> Result<Node> {
    let doc = roxmltree::Document::parse(markup)
        .map_err(|err| Error::new(format!("invalid UI markup: {err}")))?;
    let root = doc.root_element();
    let node = parse_element(root)?;
    if node.kind != ElementKind::Screen {
        return Err(Error::new(format!(
            "root element must be <screen>, found <{}>",
            node.kind.tag()
        )));
    }
    Ok(node)
}

fn parse_element(xml: roxmltree::Node<'_, '_>) -> Result<Node> {
    let tag = xml.tag_name().name();
    let kind = ElementKind::parse(tag)
        .ok_or_else(|| Error::new(format!("unsupported UI element <{tag}>")))?;

    let mut attrs = BTreeMap::new();
    let mut id = None;
    let mut classes = Vec::new();

    for attr in xml.attributes() {
        let name = attr.name();
        let value = attr.value().trim().to_owned();
        match name {
            "id" => {
                validate_rust_ident(&value, "widget id")?;
                id = Some(value.clone());
            }
            "class" => {
                for class in value.split_whitespace() {
                    validate_css_ident(class, "class")?;
                    classes.push(class.to_owned());
                }
            }
            "text" | "placeholder" | "value" | "on-press" | "visible" => {}
            _ => {
                return Err(Error::new(format!(
                    "unsupported attribute `{name}` on <{tag}>"
                )));
            }
        }
        attrs.insert(name.to_owned(), value);
    }

    if matches!(
        kind,
        ElementKind::Label | ElementKind::Button | ElementKind::TextInput | ElementKind::ListItem
    ) && xml.children().any(|child| child.is_element())
    {
        return Err(Error::new(format!("<{tag}> cannot contain child elements")));
    }

    let mut children = Vec::new();
    for child in xml.children().filter(|child| child.is_element()) {
        children.push(parse_element(child)?);
    }

    Ok(Node {
        kind,
        id,
        classes,
        attrs,
        children,
        style: Style::default(),
    })
}

#[derive(Debug, Clone, PartialEq)]
struct CssRule {
    selector: Selector,
    declarations: Style,
    order: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Selector {
    Tag(String),
    Class(String),
    Id(String),
    TagClass { tag: String, class: String },
}

impl Selector {
    fn parse(input: &str) -> Result<Self> {
        let selector = input.trim();
        if selector.is_empty() {
            return Err(Error::new("empty CSS selector"));
        }
        if selector.contains(char::is_whitespace) || selector.contains('>') || selector.contains(',')
        {
            return Err(Error::new(format!(
                "unsupported selector `{selector}`; use tag, .class, #id, or tag.class"
            )));
        }
        if let Some(id) = selector.strip_prefix('#') {
            validate_rust_ident(id, "id selector")?;
            return Ok(Self::Id(id.to_owned()));
        }
        if let Some(class) = selector.strip_prefix('.') {
            validate_css_ident(class, "class selector")?;
            return Ok(Self::Class(class.to_owned()));
        }
        if let Some((tag, class)) = selector.split_once('.') {
            if tag.is_empty() || class.is_empty() {
                return Err(Error::new(format!("invalid selector `{selector}`")));
            }
            ElementKind::parse(tag).ok_or_else(|| {
                Error::new(format!("unsupported tag selector `{tag}` in `{selector}`"))
            })?;
            validate_css_ident(class, "class selector")?;
            return Ok(Self::TagClass {
                tag: tag.to_owned(),
                class: class.to_owned(),
            });
        }
        ElementKind::parse(selector)
            .ok_or_else(|| Error::new(format!("unsupported tag selector `{selector}`")))?;
        Ok(Self::Tag(selector.to_owned()))
    }

    fn matches(&self, node: &Node) -> bool {
        match self {
            Self::Tag(tag) => node.kind.tag() == tag,
            Self::Class(class) => node.classes.iter().any(|value| value == class),
            Self::Id(id) => node.id.as_ref() == Some(id),
            Self::TagClass { tag, class } => {
                node.kind.tag() == tag && node.classes.iter().any(|value| value == class)
            }
        }
    }

    fn specificity(&self) -> u16 {
        match self {
            Self::Id(_) => 100,
            Self::TagClass { .. } => 11,
            Self::Class(_) => 10,
            Self::Tag(_) => 1,
        }
    }
}

fn parse_css(css: &str) -> Result<Vec<CssRule>> {
    let css = strip_css_comments(css);
    let mut rules = Vec::new();
    let mut rest = css.as_str();

    while !rest.trim().is_empty() {
        let open = rest
            .find('{')
            .ok_or_else(|| Error::new(format!("expected `{{` in CSS near `{}`", rest.trim())))?;
        let selector_text = rest[..open].trim();
        let after_open = &rest[open + 1..];
        let close = after_open.find('}').ok_or_else(|| {
            Error::new(format!(
                "expected `}}` for CSS rule `{selector_text}`"
            ))
        })?;
        let body = &after_open[..close];
        let selector = Selector::parse(selector_text)?;
        let declarations = parse_declarations(body, selector_text)?;
        let order = rules.len();
        rules.push(CssRule {
            selector,
            declarations,
            order,
        });
        rest = &after_open[close + 1..];
    }

    Ok(rules)
}

fn strip_css_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(ch) = chars.next() {
                if ch == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn parse_declarations(body: &str, selector: &str) -> Result<Style> {
    let mut style = Style::default();
    for decl in body.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        let (name, value) = decl.split_once(':').ok_or_else(|| {
            Error::new(format!(
                "expected `property: value` declaration in `{selector}` near `{decl}`"
            ))
        })?;
        style.set(name.trim(), value.trim(), selector)?;
    }
    Ok(style)
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Style {
    layout: Option<Direction>,
    align: Option<(Align, Align)>,
    gap: Option<u16>,
    flex_grow: Option<f32>,
    scrollable: Option<bool>,
    width: Option<SizeValue>,
    height: Option<SizeValue>,
    padding: Option<Padding>,
    background: Option<ColorValue>,
    color: Option<ColorValue>,
    border_color: Option<ColorValue>,
    border_width: Option<u16>,
    border_bottom_width: Option<u16>,
    radius: Option<u16>,
    font_size: Option<FontSizeValue>,
    max_lines: Option<u16>,
    word_wrap: Option<bool>,
    visible: Option<bool>,
}

impl Style {
    fn set(&mut self, name: &str, value: &str, selector: &str) -> Result<()> {
        match name {
            "layout" => self.layout = Some(parse_direction(value, selector)?),
            "align" => self.align = Some(parse_align_pair(value, selector)?),
            "gap" => self.gap = Some(parse_px_u16(value, name, selector)?),
            "flex-grow" => self.flex_grow = Some(parse_f32(value, name, selector)?),
            "scrollable" => self.scrollable = Some(parse_bool(value, name, selector)?),
            "width" => self.width = Some(parse_size_value(value, name, selector)?),
            "height" => self.height = Some(parse_size_value(value, name, selector)?),
            "padding" => self.padding = Some(parse_padding(value, selector)?),
            "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
                let px = parse_px_u16(value, name, selector)?;
                let mut padding = self.padding.unwrap_or_default();
                match name {
                    "padding-top" => padding.top = px,
                    "padding-right" => padding.right = px,
                    "padding-bottom" => padding.bottom = px,
                    "padding-left" => padding.left = px,
                    _ => unreachable!(),
                }
                self.padding = Some(padding);
            }
            "background" => self.background = Some(parse_color(value, name, selector)?),
            "color" => self.color = Some(parse_color(value, name, selector)?),
            "border-color" => self.border_color = Some(parse_color(value, name, selector)?),
            "border-width" => self.border_width = Some(parse_px_u16(value, name, selector)?),
            "border-bottom-width" => {
                self.border_bottom_width = Some(parse_px_u16(value, name, selector)?)
            }
            "radius" => self.radius = Some(parse_px_u16(value, name, selector)?),
            "font-size" => self.font_size = Some(parse_font_size(value, selector)?),
            "max-lines" => self.max_lines = Some(parse_u16(value, name, selector)?),
            "word-wrap" => self.word_wrap = Some(parse_bool(value, name, selector)?),
            "display" => {
                if value == "none" {
                    self.visible = Some(false);
                } else {
                    return Err(Error::new(format!(
                        "unsupported display value `{value}` in `{selector}`"
                    )));
                }
            }
            _ => {
                return Err(Error::new(format!(
                    "unsupported CSS property `{name}` in `{selector}`"
                )));
            }
        }
        Ok(())
    }

    fn merge_from(&mut self, other: &Style) {
        macro_rules! merge {
            ($field:ident) => {
                if other.$field.is_some() {
                    self.$field = other.$field.clone();
                }
            };
        }

        merge!(layout);
        merge!(align);
        merge!(gap);
        merge!(flex_grow);
        merge!(scrollable);
        merge!(width);
        merge!(height);
        merge!(padding);
        merge!(background);
        merge!(color);
        merge!(border_color);
        merge!(border_width);
        merge!(border_bottom_width);
        merge!(radius);
        merge!(font_size);
        merge!(max_lines);
        merge!(word_wrap);
        merge!(visible);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Row,
    Column,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Align {
    Start,
    Center,
    End,
    SpaceBetween,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SizeValue {
    Px(u32),
    Percent(f32),
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Padding {
    top: u16,
    right: u16,
    bottom: u16,
    left: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorValue {
    Theme(ThemeColor),
    Hex(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThemeColor {
    Primary,
    Bg,
    Surface,
    Text,
    TextSecondary,
    Accent,
    Error,
    Success,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FontSizeValue {
    Small,
    Normal,
    Large,
}

fn apply_styles(root: &mut Node, rules: &[CssRule]) {
    apply_styles_to_node(root, rules);
}

fn apply_styles_to_node(node: &mut Node, rules: &[CssRule]) {
    let mut matching = rules
        .iter()
        .filter(|rule| rule.selector.matches(node))
        .collect::<Vec<_>>();
    matching.sort_by_key(|rule| (rule.selector.specificity(), rule.order));

    let mut style = Style::default();
    if let Some(direction) = node.kind.default_direction() {
        style.layout = Some(direction);
    }
    if node.attr("visible") == Some("false") {
        style.visible = Some(false);
    }
    for rule in matching {
        style.merge_from(&rule.declarations);
    }
    node.style = style;

    for child in &mut node.children {
        apply_styles_to_node(child, rules);
    }
}

struct Codegen<'a> {
    options: &'a CompileOptions,
    node_counter: usize,
    handles: Vec<(String, String)>,
}

impl<'a> Codegen<'a> {
    fn new(options: &'a CompileOptions) -> Self {
        Self {
            options,
            node_counter: 0,
            handles: vec![("root".to_owned(), "root".to_owned())],
        }
    }

    fn generate(mut self, root: &Node) -> Result<String> {
        let mut body = String::new();
        writeln!(&mut body, "    let root_widget = {{").unwrap();
        writeln!(
            &mut body,
            "        let mut widget = ContainerWidget::default();"
        )
        .unwrap();
        self.write_style(&mut body, "widget", &root.style, ElementKind::Screen)?;
        writeln!(&mut body, "        widget").unwrap();
        writeln!(&mut body, "    }};").unwrap();
        writeln!(
            &mut body,
            "    let mut tree = UiTree::new(Widget::Container(root_widget));"
        )
        .unwrap();
        writeln!(&mut body, "    let root = tree.root();").unwrap();

        for child in &root.children {
            self.write_node(&mut body, child, "root")?;
        }

        let mut out = String::new();
        writeln!(
            &mut out,
            "// @generated by thistle-tk-ui-compiler. Do not edit by hand."
        )
        .unwrap();
        writeln!(&mut out, "#[allow(unused_imports)]").unwrap();
        writeln!(&mut out, "use thistle_tk::{{Color, UiTree, WidgetId}};").unwrap();
        writeln!(&mut out, "#[allow(unused_imports)]").unwrap();
        writeln!(&mut out, "use thistle_tk::layout::{{Align, Direction}};").unwrap();
        writeln!(&mut out, "#[allow(unused_imports)]").unwrap();
        writeln!(
            &mut out,
            "use thistle_tk::widget::{{ButtonWidget, ContainerWidget, DividerWidget, FontSize, LabelWidget, ListItemWidget, ProgressBarWidget, SizeHint, SpacerWidget, TextInputWidget, Widget}};"
        )
        .unwrap();
        writeln!(&mut out).unwrap();
        writeln!(&mut out, "#[derive(Debug, Clone, Copy)]").unwrap();
        writeln!(&mut out, "pub struct {} {{", self.options.struct_name).unwrap();
        for (field, _) in &self.handles {
            writeln!(&mut out, "    pub {field}: WidgetId,").unwrap();
        }
        writeln!(&mut out, "}}").unwrap();
        writeln!(&mut out).unwrap();
        writeln!(&mut out, "#[allow(unused_mut)]").unwrap();
        writeln!(
            &mut out,
            "pub fn {}() -> (UiTree, {}) {{",
            self.options.fn_name, self.options.struct_name
        )
        .unwrap();
        out.push_str(&body);
        writeln!(&mut out, "    (").unwrap();
        writeln!(&mut out, "        tree,").unwrap();
        writeln!(&mut out, "        {} {{", self.options.struct_name).unwrap();
        for (field, var) in &self.handles {
            writeln!(&mut out, "            {field}: {var},").unwrap();
        }
        writeln!(&mut out, "        }},").unwrap();
        writeln!(&mut out, "    )").unwrap();
        writeln!(&mut out, "}}").unwrap();
        Ok(out)
    }

    fn write_node(&mut self, out: &mut String, node: &Node, parent: &str) -> Result<String> {
        self.node_counter += 1;
        let var = node
            .id
            .as_deref()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("_node_{}", self.node_counter));
        validate_rust_ident(&var, "generated widget variable")?;

        match node.kind {
            ElementKind::Row | ElementKind::Column => {
                writeln!(out, "    let {var} = {{").unwrap();
                writeln!(out, "        let mut widget = ContainerWidget::default();").unwrap();
                self.write_style(out, "widget", &node.style, node.kind)?;
                writeln!(
                    out,
                    "        tree.add_child({parent}, Widget::Container(widget)).expect(\"generated container\")"
                )
                .unwrap();
                writeln!(out, "    }};").unwrap();
            }
            ElementKind::Label => {
                writeln!(out, "    let {var} = {{").unwrap();
                writeln!(out, "        let mut widget = LabelWidget::default();").unwrap();
                if let Some(text) = node.attr("text") {
                    writeln!(
                        out,
                        "        let _ = widget.text.push_str({});",
                        rust_string(text)
                    )
                    .unwrap();
                }
                self.write_style(out, "widget", &node.style, node.kind)?;
                writeln!(
                    out,
                    "        tree.add_child({parent}, Widget::Label(widget)).expect(\"generated label\")"
                )
                .unwrap();
                writeln!(out, "    }};").unwrap();
            }
            ElementKind::Button => {
                writeln!(out, "    let {var} = {{").unwrap();
                writeln!(out, "        let mut widget = ButtonWidget::default();").unwrap();
                if let Some(text) = node.attr("text") {
                    writeln!(
                        out,
                        "        let _ = widget.text.push_str({});",
                        rust_string(text)
                    )
                    .unwrap();
                }
                if let Some(callback) = node.attr("on-press") {
                    validate_rust_ident(callback, "on-press callback")?;
                    writeln!(out, "        // on-press: {callback}").unwrap();
                }
                self.write_style(out, "widget", &node.style, node.kind)?;
                writeln!(
                    out,
                    "        tree.add_child({parent}, Widget::Button(widget)).expect(\"generated button\")"
                )
                .unwrap();
                writeln!(out, "    }};").unwrap();
            }
            ElementKind::Divider => {
                writeln!(out, "    let {var} = {{").unwrap();
                writeln!(out, "        let mut widget = DividerWidget::default();").unwrap();
                self.write_style(out, "widget", &node.style, node.kind)?;
                writeln!(
                    out,
                    "        tree.add_child({parent}, Widget::Divider(widget)).expect(\"generated divider\")"
                )
                .unwrap();
                writeln!(out, "    }};").unwrap();
            }
            ElementKind::Spacer => {
                writeln!(out, "    let {var} = {{").unwrap();
                writeln!(out, "        let mut widget = SpacerWidget::default();").unwrap();
                self.write_style(out, "widget", &node.style, node.kind)?;
                writeln!(
                    out,
                    "        tree.add_child({parent}, Widget::Spacer(widget)).expect(\"generated spacer\")"
                )
                .unwrap();
                writeln!(out, "    }};").unwrap();
            }
            ElementKind::TextInput => {
                writeln!(out, "    let {var} = {{").unwrap();
                writeln!(out, "        let mut widget = TextInputWidget::default();").unwrap();
                if let Some(placeholder) = node.attr("placeholder") {
                    writeln!(
                        out,
                        "        let _ = widget.placeholder.push_str({});",
                        rust_string(placeholder)
                    )
                    .unwrap();
                }
                self.write_style(out, "widget", &node.style, node.kind)?;
                writeln!(
                    out,
                    "        tree.add_child({parent}, Widget::TextInput(widget)).expect(\"generated text input\")"
                )
                .unwrap();
                writeln!(out, "    }};").unwrap();
            }
            ElementKind::ListItem => {
                writeln!(out, "    let {var} = {{").unwrap();
                writeln!(out, "        let mut widget = ListItemWidget::default();").unwrap();
                if let Some(text) = node.attr("text") {
                    writeln!(
                        out,
                        "        let _ = widget.title.push_str({});",
                        rust_string(text)
                    )
                    .unwrap();
                }
                self.write_style(out, "widget", &node.style, node.kind)?;
                writeln!(
                    out,
                    "        tree.add_child({parent}, Widget::ListItem(widget)).expect(\"generated list item\")"
                )
                .unwrap();
                writeln!(out, "    }};").unwrap();
            }
            ElementKind::Progress => {
                writeln!(out, "    let {var} = {{").unwrap();
                writeln!(out, "        let mut widget = ProgressBarWidget::default();").unwrap();
                if let Some(value) = node.attr("value") {
                    let value = parse_u16(value, "value", "progress attribute")?.min(100);
                    writeln!(out, "        widget.value = {value};").unwrap();
                }
                self.write_style(out, "widget", &node.style, node.kind)?;
                writeln!(
                    out,
                    "        tree.add_child({parent}, Widget::ProgressBar(widget)).expect(\"generated progress\")"
                )
                .unwrap();
                writeln!(out, "    }};").unwrap();
            }
            ElementKind::Screen => unreachable!("nested screen is not supported"),
        }

        if let Some(id) = &node.id {
            self.handles.push((id.clone(), var.clone()));
        }

        for child in &node.children {
            self.write_node(out, child, &var)?;
        }
        Ok(var)
    }

    fn write_style(
        &self,
        out: &mut String,
        widget_var: &str,
        style: &Style,
        kind: ElementKind,
    ) -> Result<()> {
        if let Some(width) = style.width {
            writeln!(
                out,
                "        {widget_var}.common.width_hint = {};",
                size_value_code(width)
            )
            .unwrap();
        }
        if let Some(height) = style.height {
            writeln!(
                out,
                "        {widget_var}.common.height_hint = {};",
                size_value_code(height)
            )
            .unwrap();
        }
        if let Some(flex) = style.flex_grow {
            writeln!(
                out,
                "        {widget_var}.common.height_hint = SizeHint::Flex({flex:.1});"
            )
            .unwrap();
        }
        if let Some(padding) = style.padding {
            writeln!(
                out,
                "        {widget_var}.common.padding = ({}, {}, {}, {});",
                padding.left, padding.top, padding.right, padding.bottom
            )
            .unwrap();
        }
        if let Some(visible) = style.visible {
            writeln!(out, "        {widget_var}.common.visible = {visible};").unwrap();
        }
        if let Some(border_width) = style.border_width.or(style.border_bottom_width) {
            writeln!(
                out,
                "        {widget_var}.common.border_width = {border_width};"
            )
            .unwrap();
        }
        if let Some(color) = style.border_color {
            writeln!(
                out,
                "        {widget_var}.common.border_color = {};",
                color_code(color)
            )
            .unwrap();
        }
        if let Some(radius) = style.radius {
            writeln!(
                out,
                "        {widget_var}.common.border_radius = {radius};"
            )
            .unwrap();
        }
        if let Some(background) = style.background {
            writeln!(
                out,
                "        {widget_var}.common.bg_color = Some({});",
                color_code(background)
            )
            .unwrap();
        }

        match kind {
            ElementKind::Screen | ElementKind::Row | ElementKind::Column => {
                if let Some(layout) = style.layout {
                    writeln!(
                        out,
                        "        {widget_var}.direction = {};",
                        direction_code(layout)
                    )
                    .unwrap();
                }
                if let Some((main, cross)) = style.align {
                    writeln!(out, "        {widget_var}.align = {};", align_code(main)).unwrap();
                    writeln!(
                        out,
                        "        {widget_var}.cross_align = {};",
                        align_code(cross)
                    )
                    .unwrap();
                }
                if let Some(gap) = style.gap {
                    writeln!(out, "        {widget_var}.gap = {gap};").unwrap();
                }
                if let Some(background) = style.background {
                    writeln!(
                        out,
                        "        {widget_var}.bg_color = Some({});",
                        color_code(background)
                    )
                    .unwrap();
                }
                if style.scrollable == Some(true) {
                    writeln!(
                        out,
                        "        // scrollable: true (runtime scrolling support is backend-defined)"
                    )
                    .unwrap();
                }
            }
            ElementKind::Label => {
                if let Some(color) = style.color {
                    writeln!(out, "        {widget_var}.color = {};", color_code(color)).unwrap();
                }
                if let Some(font) = style.font_size {
                    writeln!(
                        out,
                        "        {widget_var}.font_size = {};",
                        font_size_code(font)
                    )
                    .unwrap();
                }
                if let Some(max_lines) = style.max_lines {
                    writeln!(out, "        {widget_var}.max_lines = {max_lines};").unwrap();
                }
                if let Some(word_wrap) = style.word_wrap {
                    writeln!(out, "        {widget_var}.word_wrap = {word_wrap};").unwrap();
                }
            }
            ElementKind::Button => {
                if let Some(background) = style.background {
                    writeln!(
                        out,
                        "        {widget_var}.bg_color = {};",
                        color_code(background)
                    )
                    .unwrap();
                }
                if let Some(color) = style.color {
                    writeln!(
                        out,
                        "        {widget_var}.text_color = {};",
                        color_code(color)
                    )
                    .unwrap();
                }
                if let Some(radius) = style.radius {
                    writeln!(out, "        {widget_var}.border_radius = {radius};").unwrap();
                }
            }
            ElementKind::TextInput => {
                if let Some(color) = style.color {
                    writeln!(
                        out,
                        "        {widget_var}.text_color = {};",
                        color_code(color)
                    )
                    .unwrap();
                }
                if let Some(color) = style.border_color {
                    writeln!(
                        out,
                        "        {widget_var}.border_color = {};",
                        color_code(color)
                    )
                    .unwrap();
                }
            }
            ElementKind::Divider => {
                if let Some(color) = style.color.or(style.border_color).or(style.background) {
                    writeln!(out, "        {widget_var}.color = {};", color_code(color)).unwrap();
                }
            }
            ElementKind::ListItem => {
                if let Some(color) = style.color {
                    writeln!(
                        out,
                        "        {widget_var}.title_color = {};",
                        color_code(color)
                    )
                    .unwrap();
                }
            }
            ElementKind::Progress | ElementKind::Spacer => {}
        }

        Ok(())
    }
}

fn parse_direction(value: &str, selector: &str) -> Result<Direction> {
    match value {
        "row" => Ok(Direction::Row),
        "column" => Ok(Direction::Column),
        _ => Err(Error::new(format!(
            "unsupported layout value `{value}` in `{selector}`"
        ))),
    }
}

fn parse_align_pair(value: &str, selector: &str) -> Result<(Align, Align)> {
    let mut parts = value.split_whitespace();
    let main = parts
        .next()
        .ok_or_else(|| Error::new(format!("missing align value in `{selector}`")))?;
    let cross = parts.next().unwrap_or("start");
    if parts.next().is_some() {
        return Err(Error::new(format!(
            "align expects one or two values in `{selector}`"
        )));
    }
    Ok((parse_align(main, selector)?, parse_align(cross, selector)?))
}

fn parse_align(value: &str, selector: &str) -> Result<Align> {
    match value {
        "start" => Ok(Align::Start),
        "center" => Ok(Align::Center),
        "end" => Ok(Align::End),
        "space-between" => Ok(Align::SpaceBetween),
        _ => Err(Error::new(format!(
            "unsupported align value `{value}` in `{selector}`"
        ))),
    }
}

fn parse_padding(value: &str, selector: &str) -> Result<Padding> {
    let values = value
        .split_whitespace()
        .map(|part| parse_px_u16(part, "padding", selector))
        .collect::<Result<Vec<_>>>()?;
    match values.as_slice() {
        [all] => Ok(Padding {
            top: *all,
            right: *all,
            bottom: *all,
            left: *all,
        }),
        [vertical, horizontal] => Ok(Padding {
            top: *vertical,
            right: *horizontal,
            bottom: *vertical,
            left: *horizontal,
        }),
        [top, right, bottom, left] => Ok(Padding {
            top: *top,
            right: *right,
            bottom: *bottom,
            left: *left,
        }),
        _ => Err(Error::new(format!(
            "padding expects 1, 2, or 4 values in `{selector}`"
        ))),
    }
}

fn parse_size_value(value: &str, name: &str, selector: &str) -> Result<SizeValue> {
    if value == "auto" {
        return Ok(SizeValue::Auto);
    }
    if let Some(px) = value.strip_suffix("px") {
        return Ok(SizeValue::Px(parse_u32(px.trim(), name, selector)?));
    }
    if let Some(percent) = value.strip_suffix('%') {
        let pct = parse_f32(percent.trim(), name, selector)?;
        return Ok(SizeValue::Percent((pct / 100.0).clamp(0.0, 1.0)));
    }
    Err(Error::new(format!(
        "`{name}` expects px, %, or auto value in `{selector}`, got `{value}`"
    )))
}

fn parse_color(value: &str, name: &str, selector: &str) -> Result<ColorValue> {
    if let Some(theme) = value
        .strip_prefix("theme(")
        .and_then(|value| value.strip_suffix(')'))
    {
        return Ok(ColorValue::Theme(match theme {
            "primary" => ThemeColor::Primary,
            "bg" | "background" => ThemeColor::Bg,
            "surface" => ThemeColor::Surface,
            "text" => ThemeColor::Text,
            "text-secondary" => ThemeColor::TextSecondary,
            "accent" => ThemeColor::Accent,
            "error" => ThemeColor::Error,
            "success" => ThemeColor::Success,
            "warning" => ThemeColor::Warning,
            _ => {
                return Err(Error::new(format!(
                    "unsupported theme color `{theme}` for `{name}` in `{selector}`"
                )));
            }
        }));
    }
    if let Some(hex) = value.strip_prefix('#') {
        if hex.len() == 6 && hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
            let value = u32::from_str_radix(hex, 16).map_err(|err| {
                Error::new(format!("invalid hex color `#{hex}` in `{selector}`: {err}"))
            })?;
            return Ok(ColorValue::Hex(value));
        }
    }
    Err(Error::new(format!(
        "`{name}` expects theme(name) or #rrggbb in `{selector}`, got `{value}`"
    )))
}

fn parse_font_size(value: &str, selector: &str) -> Result<FontSizeValue> {
    match value {
        "small" => Ok(FontSizeValue::Small),
        "normal" => Ok(FontSizeValue::Normal),
        "large" => Ok(FontSizeValue::Large),
        _ => Err(Error::new(format!(
            "font-size expects small, normal, or large in `{selector}`, got `{value}`"
        ))),
    }
}

fn parse_bool(value: &str, name: &str, selector: &str) -> Result<bool> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(Error::new(format!(
            "`{name}` expects true or false in `{selector}`, got `{value}`"
        ))),
    }
}

fn parse_px_u16(value: &str, name: &str, selector: &str) -> Result<u16> {
    let value = value.strip_suffix("px").unwrap_or(value).trim();
    parse_u16(value, name, selector)
}

fn parse_u16(value: &str, name: &str, selector: &str) -> Result<u16> {
    value.parse::<u16>().map_err(|err| {
        Error::new(format!(
            "`{name}` expects a non-negative integer in `{selector}`, got `{value}`: {err}"
        ))
    })
}

fn parse_u32(value: &str, name: &str, selector: &str) -> Result<u32> {
    value.parse::<u32>().map_err(|err| {
        Error::new(format!(
            "`{name}` expects a non-negative integer in `{selector}`, got `{value}`: {err}"
        ))
    })
}

fn parse_f32(value: &str, name: &str, selector: &str) -> Result<f32> {
    value.parse::<f32>().map_err(|err| {
        Error::new(format!(
            "`{name}` expects a number in `{selector}`, got `{value}`: {err}"
        ))
    })
}

fn size_value_code(value: SizeValue) -> String {
    match value {
        SizeValue::Px(px) => format!("SizeHint::Fixed({px})"),
        SizeValue::Percent(pct) => format!("SizeHint::Percent({pct:.4})"),
        SizeValue::Auto => "SizeHint::Auto".to_owned(),
    }
}

fn direction_code(value: Direction) -> &'static str {
    match value {
        Direction::Row => "Direction::Row",
        Direction::Column => "Direction::Column",
    }
}

fn align_code(value: Align) -> &'static str {
    match value {
        Align::Start => "Align::Start",
        Align::Center => "Align::Center",
        Align::End => "Align::End",
        Align::SpaceBetween => "Align::SpaceBetween",
    }
}

fn color_code(value: ColorValue) -> String {
    match value {
        ColorValue::Theme(ThemeColor::Primary) => "Color::Primary".to_owned(),
        ColorValue::Theme(ThemeColor::Bg) => "Color::Background".to_owned(),
        ColorValue::Theme(ThemeColor::Surface) => "Color::Surface".to_owned(),
        ColorValue::Theme(ThemeColor::Text) => "Color::Text".to_owned(),
        ColorValue::Theme(ThemeColor::TextSecondary) => "Color::TextSecondary".to_owned(),
        ColorValue::Theme(ThemeColor::Accent) => "Color::Accent".to_owned(),
        ColorValue::Theme(ThemeColor::Error) => "Color::Error".to_owned(),
        ColorValue::Theme(ThemeColor::Success) => "Color::Success".to_owned(),
        ColorValue::Theme(ThemeColor::Warning) => "Color::Warning".to_owned(),
        ColorValue::Hex(hex) => format!("Color::from_hex(0x{hex:06X})"),
    }
}

fn font_size_code(value: FontSizeValue) -> &'static str {
    match value {
        FontSizeValue::Small => "FontSize::Small",
        FontSizeValue::Normal => "FontSize::Normal",
        FontSizeValue::Large => "FontSize::Large",
    }
}

fn rust_string(value: &str) -> String {
    format!("{value:?}")
}

fn validate_rust_ident(value: &str, what: &str) -> Result<()> {
    let mut chars = value.chars();
    match chars.next() {
        Some(ch) if ch == '_' || ch.is_ascii_alphabetic() => {}
        _ => {
            return Err(Error::new(format!(
                "{what} `{value}` must start with '_' or an ASCII letter"
            )));
        }
    }
    if chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric()) {
        if is_rust_reserved_word(value) {
            Err(Error::new(format!(
                "{what} `{value}` is a reserved Rust keyword"
            )))
        } else {
            Ok(())
        }
    } else {
        Err(Error::new(format!(
            "{what} `{value}` may only contain ASCII letters, digits, and '_'"
        )))
    }
}

fn is_rust_reserved_word(value: &str) -> bool {
    matches!(
        value,
        "Self"
            | "abstract"
            | "as"
            | "async"
            | "await"
            | "become"
            | "box"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "do"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "final"
            | "fn"
            | "for"
            | "gen"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "macro"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "override"
            | "priv"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "try"
            | "type"
            | "typeof"
            | "union"
            | "unsafe"
            | "unsized"
            | "use"
            | "virtual"
            | "where"
            | "while"
            | "yield"
    )
}

fn validate_css_ident(value: &str, what: &str) -> Result<()> {
    let mut chars = value.chars();
    match chars.next() {
        Some(ch) if ch == '-' || ch == '_' || ch.is_ascii_alphabetic() => {}
        _ => {
            return Err(Error::new(format!(
                "{what} `{value}` must start with '-', '_' or an ASCII letter"
            )));
        }
    }
    if chars.all(|ch| ch == '-' || ch == '_' || ch.is_ascii_alphanumeric()) {
        Ok(())
    } else {
        Err(Error::new(format!(
            "{what} `{value}` may only contain ASCII letters, digits, '_' and '-'"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_str_eq;

    const MARKUP: &str = include_str!("../fixtures/weather.ui.xml");
    const CSS: &str = include_str!("../fixtures/weather.css");

    #[test]
    fn generates_launcher_style_rust() {
        let out = compile_to_rust(MARKUP, CSS, &CompileOptions::new("WeatherUi", "build_weather"))
            .expect("compile UI");

        assert!(out.contains("pub struct WeatherUi"));
        assert!(out.contains("pub content: WidgetId"));
        assert!(out.contains("pub temperature: WidgetId"));
        assert!(out.contains("pub refresh: WidgetId"));
        assert!(out.contains("widget.direction = Direction::Row;"));
        assert!(out.contains("widget.font_size = FontSize::Large;"));
        assert!(out.contains("// on-press: refresh"));
        assert!(out.contains("Color::Primary"));
    }

    #[test]
    fn rejects_unsupported_css() {
        let err = compile_to_rust(
            "<screen><button text=\"Go\"/></screen>",
            "button { position: absolute; }",
            &CompileOptions::new("TestUi", "build_test"),
        )
        .unwrap_err();

        assert!(err.message().contains("unsupported CSS property `position`"));
    }

    #[test]
    fn rejects_rust_keyword_ids() {
        let err = compile_to_rust(
            r#"<screen><label id="type" text="Bad"/></screen>"#,
            "",
            &CompileOptions::new("TestUi", "build_test"),
        )
        .unwrap_err();

        assert!(err.message().contains("reserved Rust keyword"));
    }

    #[test]
    fn later_more_specific_rules_win() {
        let out = compile_to_rust(
            r#"<screen><button id="cta" class="primary" text="Go"/></screen>"#,
            r#"
button { color: theme(text); }
.primary { color: theme(bg); }
#cta { color: #112233; }
"#,
            &CompileOptions::new("TestUi", "build_test"),
        )
        .expect("compile UI");

        assert!(out.contains("widget.text_color = Color::from_hex(0x112233);"));
    }

    #[test]
    fn tag_class_rule_beats_later_class_rule() {
        let out = compile_to_rust(
            r#"<screen><button class="primary" text="Go"/></screen>"#,
            r#"
button.primary { color: #112233; }
.primary { color: theme(bg); }
"#,
            &CompileOptions::new("TestUi", "build_test"),
        )
        .expect("compile UI");

        assert!(out.contains("widget.text_color = Color::from_hex(0x112233);"));
    }

    #[test]
    fn strips_css_comments() {
        assert_str_eq!(
            strip_css_comments("a { color: red; /* nope */ background: blue; }"),
            "a { color: red;  background: blue; }"
        );
    }

    #[test]
    fn compiles_files_to_output_path() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("thistle-tk-ui-compiler-{stamp}"));
        let markup_path = dir.join("weather.ui.xml");
        let css_path = dir.join("weather.css");
        let out_path = dir.join("generated").join("weather_ui.rs");

        std::fs::create_dir_all(&dir).expect("create temp dir");
        std::fs::write(&markup_path, MARKUP).expect("write markup");
        std::fs::write(&css_path, CSS).expect("write css");

        compile_files_to_path(
            &markup_path,
            &css_path,
            &out_path,
            &CompileOptions::new("WeatherUi", "build_weather"),
        )
        .expect("compile files");

        let generated = std::fs::read_to_string(&out_path).expect("read generated");
        assert!(generated.contains("pub struct WeatherUi"));

        std::fs::remove_dir_all(&dir).expect("remove temp dir");
    }

    #[test]
    fn compiles_for_build_script_into_out_dir() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("thistle-tk-ui-build-{stamp}"));
        let out_dir = dir.join("out");
        let markup_path = dir.join("weather.ui.xml");
        let css_path = dir.join("weather.css");

        std::fs::create_dir_all(&out_dir).expect("create temp dirs");
        std::fs::write(&markup_path, MARKUP).expect("write markup");
        std::fs::write(&css_path, CSS).expect("write css");

        let previous_out_dir = std::env::var_os("OUT_DIR");
        std::env::set_var("OUT_DIR", &out_dir);
        let generated_path = compile_for_build_script(
            &markup_path,
            &css_path,
            "weather_ui.rs",
            &CompileOptions::new("WeatherUi", "build_weather"),
        )
        .expect("compile build script");
        match previous_out_dir {
            Some(value) => std::env::set_var("OUT_DIR", value),
            None => std::env::remove_var("OUT_DIR"),
        }

        assert_eq!(generated_path, out_dir.join("weather_ui.rs"));
        let generated = std::fs::read_to_string(generated_path).expect("read generated");
        assert!(generated.contains("pub fn build_weather()"));

        std::fs::remove_dir_all(&dir).expect("remove temp dir");
    }
}

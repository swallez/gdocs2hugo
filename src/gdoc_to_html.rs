


// Reference doc: https://developers.google.com/docs/api/reference/rest/v1/documents#Document

use std::collections::{BTreeMap, HashMap};
use std::fmt::{Display, Formatter, Write as FmtWrite};
use std::ops::AddAssign;
use std::path::Path;
use google_docs1::api as docs;
use crate::html::HtmlConsumer;
use anyhow::{anyhow, bail};


pub fn read(p: impl AsRef<Path>) -> anyhow::Result<docs::Document> {
    let file = std::fs::File::open(p.as_ref())?;
    let doc: docs::Document = serde_json::from_reader(file)?;
    Ok(doc)
}

//-------------------------------------------------------------------------------------------------
// Image rewriter

#[derive(Debug)]
pub struct ImageReference<'a> {
    pub id: &'a str,
    pub src: &'a str,
}

pub struct ImageRewriter<'r, C: HtmlConsumer> {
    resolver: Box<dyn FnMut(&ImageReference) -> String + 'r>,
    next: C,
}

impl <'r, C: HtmlConsumer> ImageRewriter<'r, C> {
    pub fn new(resolver: impl FnMut(&ImageReference) -> String + 'r, next: C) -> Self {
        ImageRewriter {
            resolver: Box::new(resolver),
            next,
        }
    }
}

impl <C: HtmlConsumer> HtmlConsumer for ImageRewriter<'_, C> {
    fn start_document(&mut self) -> anyhow::Result<()> {
        self.next.start_document()
    }

    fn start_element(&mut self, name: &str, classes: Vec<&str>, style: HashMap<&str, &str>, attrs: HashMap<&str, &str>) -> anyhow::Result<()> {
        if name == "img" {
            let src = attrs.get("src").ok_or(anyhow!("<img> tag with no 'src' attribute"))?;
            let id = attrs.get("id").ok_or(anyhow!("<img> tag with no 'id' attribute"))?;

            let img_ref = ImageReference {
                id: id,
                src: src
            };
            let new_src: String = (self.resolver)(&img_ref);
            let mut new_attrs = attrs;
            new_attrs.insert("src", new_src.as_str());
            self.next.start_element(name, classes, style, new_attrs)
        } else {
            self.next.start_element(name, classes, style, attrs)
        }
    }

    fn text(&mut self, text: &str) -> anyhow::Result<()> {
        self.next.text(text)
    }

    fn end_element(&mut self, name: &str) -> anyhow::Result<()> {
        self.next.end_element(name)
    }

    fn end_document(&mut self) -> anyhow::Result<()> {
        self.next.end_document()
    }
}

//-------------------------------------------------------------------------------------------------

// See https://kramdown.gettalong.org/syntax.html#inline-attribute-lists

#[derive(Default)]
struct InlineAttributes {
    id: Option<String>,
    pub classes: Vec<String>,
    attrs: BTreeMap<String, String>,
}

enum AttrTag {
    Start(InlineAttributes),
    End,
}


// use nom::character::complete::{ alphanumeric1, alpha1, space0, space1 };
// use nom::combinator::{value, verify };
// use nom::multi::{ many1 };
// use nom::IResult;

impl AttrTag {
    pub fn parse_para<'a>(para: &'a google_docs1::api::Paragraph) -> anyhow::Result<Option<(AttrTag, &'a str)>> {
        if let Some(ref elts) = para.elements {
            if !(elts.is_empty()) {
                if let Some(ref text) = elts[0].text_run {
                    if let Some(ref content) = text.content {
                        return AttrTag::parse(content);
                    }
                }
            }
        }
        Ok(None)
    }

    pub fn parse<'a>(text: &'a str) -> anyhow::Result<Option<(AttrTag, &'a str)>> {
        let mut text = text;
        if !text.starts_with("{: ") {
            if text.starts_with("{::}") {
                return Ok(Some((AttrTag::End, &text[4..])));
            }
            return Ok(None);
        }

        text = &text[3..];

        if let Some(end) = text.find("}") {
            let mut attrs = InlineAttributes::default();
            let parts = (&text[..end]).trim().split(' ');

            attrs.classes = parts.map(String::from).collect();

            return Ok(Some((AttrTag::Start(attrs), &text[end+1..])));

        } else {
            bail!("Missing closing '}}'");
        }
    }
}

impl Display for InlineAttributes {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // FIXME escape values
        if let Some(ref id) = self.id {
            write!(f, " id='{}'", id)?;
        }
        if !self.classes.is_empty() {
            write!(f, " class='{}'", self.classes.join(" "))?;
        }
        for (k, v) in &self.attrs {
            write!(f, " {}='{}'", k, v)?;
        }
        Ok(())
    }
}

//-------------------------------------------------------------------------------------------------
// GDocs HTML renderer

// We need <'r> because the closure is boxed to be stored in the HtmlRenderer struct and we want
// to allow it to refer its scope mutably so that the caller can accumulate images referenced in
// a document and process them after conversion.
// Callbacks and lifetimes https://stackoverflow.com/questions/41081240/idiomatic-callbacks-in-rust


pub fn render(
    doc: &docs::Document,
    ) -> anyhow::Result<String> {
    let mut renderer = HtmlRenderer {
        doc,
        html: String::new(),
        tags: Vec::new(),
        last_is_nl: false,
    };

    renderer.format_doc()?;
    return Ok(renderer.html);
}

struct HtmlRenderer <'a> {
    // Input
    doc: &'a docs::Document,

    // State
    tags: Vec<&'a str>,

    // Output
    html: String,

    last_is_nl: bool,
}

impl AddAssign<&str> for HtmlRenderer<'_> {
    fn add_assign(&mut self, rhs: &str) {
        self.add_html_text(rhs)
    }
}

impl AddAssign<String> for HtmlRenderer<'_> {
    fn add_assign(&mut self, rhs: String) {
        self.add_html_text(rhs.as_str());
    }
}

impl AddAssign<&Option<String>> for HtmlRenderer<'_> {
    fn add_assign(&mut self, rhs: &Option<String>) {
        if let Some(rhs) = rhs {
            self.add_html_text(rhs);
        }
    }
}

impl AddAssign<&String> for HtmlRenderer<'_> {
    fn add_assign(&mut self, rhs: &String) {
        self.add_html_text(rhs.as_str());
    }
}

#[derive(Default)]
struct Indent {
    depth: usize,
    magnitude: f64,
}

impl <'a> HtmlRenderer<'a> {

    fn start_tag(&mut self, tag: &'a str, attrs: &[(&str, &str)]) {
        let mut write_it = || -> std::fmt::Result {
            self.html.write_str("<")?;
            self.html.write_str(tag)?;
            for attr in attrs {
                if !attr.1.is_empty() {
                    write!(self.html, " {}=\"", attr.0)?;
                    crate::html::write_escaped_fmt(&mut self.html, attr.1, true)?;
                    self.html.write_char('"')?;
                }
            }
            self.html.write_char('>')?;
            self.tags.push(tag);
            Ok(())
        };

        // We write to a string, this should never fail (unless we OOM)
        write_it().expect("Problem opening tag?");
    }

    fn end_tag(&mut self) {
        let tag = self.tags.pop().unwrap();
        *self += "</";
        *self += tag;
        *self += ">";
    }

    fn add_html_text(&mut self, text: &str) {
        self.html.push_str(text);
//        crate::html::write_escaped_fmt(&mut self.html, text, false)?;
        if !text.is_empty() {
            self.last_is_nl = text.chars().last() == Some('\n');
        }
    }

    fn end_tag_nl(&mut self) {
        self.end_tag();
        self.nl();
    }

    fn nl(&mut self) {
        if !self.html.ends_with('\n') {
            *self += "\n";
        }
    }

    fn content(&mut self, text: &str) {
        let mut text = text;
        // Note: we never append text that ends with an \nl
        if text.ends_with('\n') {
            text = &text[.. text.len() - 1]
        }

        let mut split = text.split('\u{000B}');
        if let Some(first) = split.next() {
            crate::html::write_escaped_fmt(&mut self.html, first, false).unwrap();
            //self.add_html_text(first);
            for next in split {
                *self += "<br>\n";
                crate::html::write_escaped_fmt(&mut self.html, next, false).unwrap();
                //self.add_html_text(next);
            }
        }
    }

    /// Result should be escaped, and safe for inclusion in html
    fn convert_url(&self, url: impl AsRef<str>) -> String {
        // TODO
        url.as_ref().to_string()
    }

    fn format_doc(&mut self) -> anyhow::Result<()> {
        self.start_tag("html", &[]);
        self.nl();

        self.start_tag("head", &[]);
        self.nl();
        if let Some(title) = &self.doc.title {
            self.start_tag("title", &[]);
            *self += title;
            self.end_tag_nl();
        }
        self.end_tag_nl();
        self.start_tag("body", &[]);

        self.format_body()?;
        self.format_footnotes();

        // TODO
        // - doc.inline_objects

        // ???
        // - doc.document_id
        // - doc.document_style
        // - doc.inline_objects
        // - doc.lists
        // - doc.named_styles
        // - doc.positioned_objects

        // Ignored
        // - doc.headers
        // - doc.footers
        // - doc.named_ranges
        // - doc.suggested_document_style_changes
        // - doc.suggested_named_styles_changes
        // - doc.suggestions_view_mode

        self.end_tag_nl();
        self.end_tag_nl();

        Ok(())
    }

    /// The document body.
    fn format_body(&mut self) -> anyhow::Result<()> {
        if let Some(body) = &self.doc.body {
            self.format_structural_elements(&body.content)?;
        }

        Ok(())
    }

    fn format_structural_elements(&mut self, elements: &'a Option<Vec<docs::StructuralElement>>) -> anyhow::Result<()> {
        // TODO: identify inline attribute lists.
        // See https://kramdown.gettalong.org/syntax.html#inline-attribute-lists
        let mut indent = Indent::default();
        if let Some(elements) = elements {
            for elt in elements {
                self.format_structural_element(elt, &mut indent)?;
            }
        }
        for _ in 0..indent.depth {
            self.end_tag_nl();
        }

        Ok(())
    }

    fn format_structural_element(&mut self, elt: &'a docs::StructuralElement, indent: &mut Indent) -> anyhow::Result<()>{
        if let Some(para) = &elt.paragraph {
            self.format_paragraph(&para, indent)?;

        } else if let Some(table) = &elt.table {
            self.format_table(&table)?;

        } else if let Some(section_break) = &elt.section_break {
            self.format_section_break(section_break);

        } else if let Some(toc) = &elt.table_of_contents {
            self.format_table_of_contents(toc)?;

        } else {
            unimplemented!("Unknown structural element type {:?}", elt);
        }

        Ok(())
    }

    fn format_table_of_contents(&mut self, toc: &'a docs::TableOfContents) -> anyhow::Result<()> {
        self.start_tag("div", &[("class", "table-of-contents")]);
        self.format_structural_elements(&toc.content)?;
        self.end_tag_nl();
        Ok(())
    }

    fn format_section_break(&mut self, _section: &docs::SectionBreak) {
        // FIXME
        // Ignore for now
        // The interesting bits: column properties (multi-column sections)
        // Should create a <div> containing following sibling elements until the next section break
    }

    fn get_shortcode(&mut self, para: &'a docs::Paragraph) -> Option<String> {
        let mut text = String::new();

        if let Some(ref elts) = para.elements {
            for elt in elts {
                if let Some(ref text_run) = elt.text_run {
                    if let Some(ref content) = text_run.content {
                        text += content;
                    }
                }
            }
        }

        let trimmed = text.trim();

        if (trimmed.starts_with("{{") && trimmed.ends_with("}}")) ||
            (trimmed.starts_with("{:") && trimmed.ends_with(":}")) {
            Some(text)
        } else {
            None
        }
    }

    fn write_shortcode(txt: &str, out: &mut impl std::fmt::Write) -> anyhow::Result<()> {
        // GDocs likes fancy quotes...
        let txt = txt.replace('”', "\"").replace('’', "'");

        for mut code in txt.split("{{") {
            if code.len() == 0 {
                // Beginning of first shortcode
                continue;
            }
            // Remove ending delimiters and any remaining whitespace
            // and also allow either '{{' or '{{<' notation.
            code = code.trim_start_matches("<");
            code = code.trim_end();
            code = code.trim_end_matches("}}");
            code = code.trim_end_matches(">");
            code = code.trim();

            // Find command
            let (cmd, args) = code.split_once(' ').unwrap_or_else(|| (code, ""));

            if cmd == "html" {
                out.write_str(args)?;
            } else {
                // Write it as a comment, the serializer will then write it verbatim
                write!(out, "<!--{{{{< {} >}}}}-->", code)?;
            }
        }

        Ok(())
    }

    /// A paragraph is a range of content that is terminated with a newline character.
    fn format_paragraph(&mut self, para: &'a docs::Paragraph, indent: &mut Indent) -> anyhow::Result<()> {

        // Find this paragraph's nesting level and add/close <ul>s accordingly
        let cur_depth = indent.depth;
        let mut new_depth;

        let cur_magnitude = indent.magnitude;
        let mut new_magnitude: f64 = 0.0;

        if let Some(bullet) = &para.bullet {
            // nesting level starts at 0, and 0 is represented as None.
            new_depth = bullet.nesting_level.unwrap_or(0) as usize + 1;
        } else {
            new_depth = 0;
        }

        if let Some(style) = &para.paragraph_style {
            if let Some(indent) = &style.indent_start {
                if let Some(magnitude) = &indent.magnitude {
                    new_magnitude = *magnitude;
                    if new_depth == 0 && *magnitude == cur_magnitude {
                        // FIXME only handles paragraph following a list item, not general indentation
                        new_depth = cur_depth;
                    }
                }
            }
        }

        for _ in cur_depth..new_depth {
            // FIXME: distinguish ul/ol and list-style-type
            self.nl();
            self.start_tag("ul", &[]);
            self.nl();
        }

        for _ in new_depth..cur_depth {
            self.end_tag();
            self.nl();
        }

        indent.depth = new_depth;
        indent.magnitude = new_magnitude;

        if let Some(short_code) = self.get_shortcode(para) {
            if let Some(tag) = AttrTag::parse_para(para)? {
                // gdoc2hugo shortcode
                let attrs = tag.0;
                self.nl();
                match attrs {
                    AttrTag::Start(attrs) => {
                        *self += "<div";
                        *self += attrs.to_string();
                        *self += ">";
                    },
                    AttrTag::End => {
                        self.nl();
                        *self += "</div>";
                    }
                };
                self.nl();

                return Ok(());
            } else {
                self.nl();
                Self::write_shortcode(&short_code, &mut self.html)?;
                self.nl();
                return Ok(());
            }
        }

        // Ignored:
        // - para.suggested_paragraph_style_changes
        // - para.suggested_bullet_changes
        // - para.suggested_positioned_object_ids

        let mut tag = "p";
        let mut class: &'a str = "";
        let mut id = "";

        // para.positioned_object_ids
        let mut style_attr = String::new();

        if let Some(style) = &para.paragraph_style {
            if let Some(name) = &style.named_style_type {
                match name.as_str() {
                    "NORMAL_TEXT" => (),
                    "HEADING_1" => tag = "h1",
                    "HEADING_2" => tag = "h2",
                    "HEADING_3" => tag = "h3",
                    "HEADING_4" => tag = "h4",
                    _ => class = name, // "TITLE" & "SUBTITLE"
                }
            }
            if let Some(align) = &style.alignment {
                style_attr += match align.as_str() {
                    "START" => "text-align:start;",
                    "END" => "text-align:end;",
                    "CENTER" => "text-align:center;",
                    //"JUSTIFIED" => "text-align:justify;",
                    _ => "", // "UNSPECIFIED" or other value
                };
            }

            if let Some(ref heading) = style.heading_id {
                id = heading;
            }
        }

        if let Some(_bullet) = &para.bullet {
            tag = "li";
        }

        self.nl();
        self.start_tag(tag, &[("id", id), ("class", class), ("style", &style_attr)]);

        if let Some(elements) = &para.elements {
            for elt in elements {
                self.format_paragraph_element(elt);
            }
        }

        self.end_tag_nl();

        Ok(())
    }

    fn format_paragraph_element(&mut self, elt: &docs::ParagraphElement) {
        // Ignored
        // elt.start_index
        // elt.end_index

        // Union
        if let Some(text) = &elt.text_run {
            self.format_text_run(&text);

        } else if let Some(auto_text) = &elt.auto_text {
            unimplemented!("Auto text {}", auto_text.type_.as_ref().map_or("", |s| s.as_str()));

        } else if let Some(_page_break) = &elt.page_break {
            // Ignore

        } else if let Some(_column_break) = &elt.column_break {
            // Apply to an entire section
            unimplemented!("Column break");

        } else if let Some(_footnote_ref) = &elt.footnote_reference {

        } else if let Some(_hr) = &elt.horizontal_rule {
            *self += "<hr>\n";

        } else if let Some(_equation) = &elt.equation {
            unimplemented!("Equations");

        } else if let Some(inline_obj) = &elt.inline_object_element {
            // TODO
            // Lookup inlineObj.inline_object_id in self.doc.inline_objects
            let id = inline_obj.inline_object_id.as_ref().unwrap();
            let obj: &docs::EmbeddedObject = self.doc
                .inline_objects.as_ref().unwrap()
                .get(id).as_ref().unwrap()
                .inline_object_properties.as_ref().unwrap()
                .embedded_object.as_ref().unwrap();

            self.format_embedded_object(id, obj);

        } else if let Some(person) = &elt.person {
            *self += &person.person_properties.as_ref().unwrap().name;

        } else if let Some(_link) = &elt.rich_link {
            // TODO
        } else {
            unimplemented!("Unknown paragraph element {:?}", elt);
        }
    }

    fn format_embedded_object(&mut self, mut id: &str, obj: &docs::EmbeddedObject) {
        // Can be either an embedded drawing or an image
        if let Some(img) = &obj.image_properties {

            let width = dimension_to_px(obj.size.as_ref().unwrap().width.as_ref().unwrap());
            let height = dimension_to_px(obj.size.as_ref().unwrap().height.as_ref().unwrap());
            let mut span_style = format!("width:{:.2}px;height:{:.2}px;", width, height);
            if let Some(angle) = &img.angle {
                span_style += &format!("transform:rotate({:.3}rad) translateZ(0px);", angle);
            }

            let mut img_style = String::new();

            let mut offset_top = 0.0;
            let mut offset_bottom = 0.0;
            let mut offset_left = 0.0;
            let mut offset_right = 0.0;
            if let Some(crop) = &img.crop_properties {
                offset_top = crop.offset_top.unwrap_or(0.0) as f64;
                offset_bottom = crop.offset_bottom.unwrap_or(0.0) as f64;
                offset_left = crop.offset_left.unwrap_or(0.0) as f64;
                offset_right = crop.offset_right.unwrap_or(0.0) as f64;

                if let Some(angle) = crop.angle {
                    img_style += &format!("transform:rotate({:.3}rad) translateZ(0px);", angle);
                }
            }

            let img_width = width/(1.0 - offset_left - offset_right);
            let img_height = height/(1.0 - offset_top - offset_bottom);

            img_style += &format!("width:{:.2}px;height:{:.2}px;margin-left:{:.2}px;margin-top:{:.2}px;",
                img_width, img_height,
                - img_width * offset_left,
                - img_height * offset_top
            );

            self.start_tag("span", &[("style", &span_style)]);

            // Image ids are "kix.<id>" where <id> seems to be 12 base-36 chars
            if id.starts_with("kix.") {
                id = &id["kix.".len()..];
            }

            *self += "<img id='";
            *self += id;
            *self += "' style='";
            *self += img_style;
            *self += "' src='";
            *self += img.content_uri.as_ref().unwrap();
            // let new_uri = &(self.image_resolver)(&ImageReference {
            //     doc_id: self.doc.document_id.as_ref().unwrap(),
            //     image_id: id,
            //     url: img.content_uri.as_ref().unwrap(),
            // });
            // *self += new_uri;
            *self += "'>";

            self.end_tag();

        }
    }

    fn format_text_run(&mut self, text: &docs::TextRun) {
        let mut style_attr = String::new();
        let mut elts = Vec::<&str>::new();

        let mut link: Option<String> = None;

        if let Some(style) = &text.text_style {
            if let Some(style_link) = &style.link {
                link = style_link.url.clone();
                // NOTE: Links have foreground-color and underline styles. We'll skip them
                // below to avoid interfering with the CSS style.
                // We may want to keep these however if they're the same as the ones of the
                // previous/next text runs (link in a styled paragraph)

                if let Some(ref heading) = style_link.heading_id {
                    link = Some(format!("#{}", heading));
                }

                if let Some(ref bkm) = style_link.bookmark_id {
                    link = Some(format!("#{}", bkm));
                }
            }

            if style.bold.unwrap_or(false) {
                elts.push("strong");
            }
            if style.italic.unwrap_or(false) {
                elts.push("em")
            }
            if style.strikethrough.unwrap_or(false) {
                elts.push("del");
            }
            if link.is_none() && style.underline.unwrap_or(false) {
                style_attr += "text-decoration: underline;"
            }
            if style.small_caps.unwrap_or(false) {
                style_attr += "font-variant: small-caps;";
            }
            if let Some(offset) = &style.baseline_offset {
                match offset.as_str() {
                    // The text's baseline offset is inherited from the parent.
                    "BASELINE_OFFSET_UNSPECIFIED" => (),
                    "SUPERSCRIPT" => elts.push("sup"),
                    "SUBSCRIPT" => elts.push("sub"),
                    // The text is not vertically offset.
                    "NONE" => (),
                    _ => (),
                }
            }

            if link.is_none() {
                add_color("color", &mut style_attr, &style.foreground_color);
            }
            add_color("background-color", &mut style_attr, &style.background_color);

            // FIXME
            // - style.weighted_font_family
            //   seems it's never defined on textRun, only on namedStyle
            // - style.font_size
        }

        // Start <a>
        if let Some(ref url) = link {
            *self += "<a href='";
            if url.starts_with('#') {
                *self += url;
            } else {
                *self +=  self.convert_url(url);
            }
            *self += "'>";
        }

        // Start <del>, <sup> etc.
        for elt in &elts {
            self.start_tag(elt, &[]);
        }

        // Start <span> if we have custom styles. We do not add style on a surrounding tag, as it
        // may conflict with that tag's default styling (e.g. strike-through in <del> may be
        // overriden by an underlined style)
        if style_attr.len() > 0 {
            self.start_tag("span", &[("style", &style_attr)]);
        }

        // Content
        if let Some(content) = &text.content {
            self.content(&content);
        }

        // Close <span>
        if style_attr.len() > 0 {
            self.end_tag();
        }

        // Close <del>, <sup>, etc.
        for _elt in elts.iter() {
            self.end_tag();
        }

        // Close <a>
        if link.is_some() {
            *self += "</a>";
        }
    }

    fn format_table(&mut self, table: &'a docs::Table) -> anyhow::Result<()> {
        *self += "<table>\n";

        // TODO: check if first line style is different from others, and then make it a <thead>
        *self += "<tbody>\n";

        // Merged table cells (colspan or rowspan > 1) are still present as empty cells that must
        // be ignored. This vector has the width of the table and for each column contains the
        // number of times it must be skipped, either because we've seen a colspan in the same
        // row or a rowspan in the same column.
        let mut skips = Vec::<usize>::new();

        if let Some(rows) = &table.table_rows {
            for row in rows {
                *self += "<tr>\n";
                if let Some(cells) = &row.table_cells {
                    if skips.len() == 0 {
                        // First line: resize to the width of the table
                        skips.resize(cells.len(), 0);
                    }

                    for (col, cell) in cells.iter().enumerate() {
                        if skips[col] > 0 {
                            skips[col] -= 1;
                            continue;
                        }

                        // TODO: cell.table_cell_style
                        let mut colspan = 0;
                        let mut rowspan = 0;
                        if let Some(style) = &cell.table_cell_style {
                            colspan = style.column_span.unwrap_or(0) as usize;
                            rowspan = style.row_span.unwrap_or(0) as usize;
                        }
                        *self += "<td";
                        if colspan > 1 {
                            // Skip some following cells on this row
                            for i in col+1..col+colspan {
                                skips[i] += 1;
                            }
                            *self += format!(" colspan='{}'", colspan);
                        }
                        if rowspan > 1 {
                            // Skip some rows below
                            skips[col] += rowspan - 1;
                            *self += format!(" rowspan='{}'", rowspan);
                        }
                        *self += ">\n";

                        // Cell content
                        self.format_structural_elements(&cell.content)?;

                        *self += "</td>\n";
                    }
                }
                self.nl();
                *self += "</tr>\n";
            }
        }
        *self += "</tbody>\n";
        *self += "</table>\n";

        Ok(())
    }

    fn format_footnotes(&mut self) {
        // TODO

        // Some text with a footnote.<sup><a href="#fn1" id="ref1">1</a></sup>
        //
        // <sup id="fn1">1. [Text of footnote 1]<a href="#ref1" title="Jump back to
        //      footnote 1 in the text.">↩</a></sup>
        // https://www.w3.org/TR/dpub-aria-1.1/#doc-backlink
        // https://www.w3.org/TR/dpub-aria-1.1/#doc-endnotes
        // https://www.w3.org/TR/dpub-aria-1.1/#doc-noteref
        // https://kittygiraudel.com/2020/11/24/accessible-footnotes-and-a-bit-of-react/#footnotes-ref

    }

}

fn add_color(name: &str, style_attr: &mut String, color: &Option<docs::OptionalColor>) {
    // Optional color is a weird russian puppet. And also:
    // > If set, this will be used as an opaque color. If unset, this represents a
    // > transparent color.
    // -- What is the transparent color if we don't have rgba information?
    if let Some(color) = &color {
        if let Some(color) = &color.color {
            if let Some(color) = &color.rgb_color {
                // End of russian puppets...
                let style = format!(
                    "{}:rgb({:.0}%,{:.0}%,{:.0}%);",
                    name,
                    // Some or all colors are sometimes null.
                    color.red.unwrap_or(0.0)*100.0,
                    color.green.unwrap_or(0.0)*100.0,
                    color.blue.unwrap_or(0.0)*100.0
                );
                *style_attr += &style;
            }
        }
    }
}

fn dimension_to_px(dimension: &docs::Dimension) -> f64 {
    let unit = dimension.unit.as_ref().unwrap();
    let magnitude = dimension.magnitude.as_ref().unwrap();
    if unit.as_str() == "PT" {
        return *magnitude / 0.75;
    }
    panic!("Unknown unit {}", unit);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_attr() -> anyhow::Result<()> {
        let x = AttrTag::parse("{: x y z }blah")?;

        match x {
            Some((AttrTag::Start(attrs), s)) => {
                assert_eq!(attrs.to_string(), " class='x y z'");
                assert_eq!(s, "blah");
                return Ok(())
            },
            _ => return Err(anyhow!("expecting a start tag")),
        }
    }

    #[test]
    fn test_write_short_code() -> anyhow::Result<()> {
        let mut out = String::new();
        HtmlRenderer::write_shortcode("{{ html <div class='bar'> }}", &mut out).expect("write");
        assert_eq!("<div class='bar'>", out);

        let mut out = String::new();
        HtmlRenderer::write_shortcode("{{ html <div class=”row”> }}", &mut out).expect("write");
        assert_eq!("<div class=\"row\">", out);

        let mut out = String::new();
        HtmlRenderer::write_shortcode(r#"{{ youtube id="xyz" }}"#, &mut out).expect("write");
        assert_eq!(r#"<!--{{< youtube id="xyz" >}}-->"#, out);

        let mut out = String::new();
        HtmlRenderer::write_shortcode("{{ html <div class='bar'> }} \u{0B} {{ youtube id='xyz' }}", &mut out).expect("write");
        assert_eq!(r#"<div class='bar'><!--{{< youtube id='xyz' >}}-->"#, out);

        Ok(())
    }
}

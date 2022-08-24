


// Reference doc: https://developers.google.com/docs/api/reference/rest/v1/documents#Document

use std::collections::HashMap;
use std::ops::AddAssign;
use std::path::Path;
use google_docs1::api as docs;
use itertools::Itertools;
use crate::html::HtmlConsumer;
use anyhow::anyhow;


pub fn read(p: impl AsRef<Path>) -> anyhow::Result<docs::Document> {
    let file = std::fs::File::open(p.as_ref())?;
    let doc: docs::Document = serde_json::from_reader(file)?;
    Ok(doc)
}

//-------------------------------------------------------------------------------------------------
// Image rewriter

#[derive(Debug)]
pub struct ImageReference<'a> {
    pub doc_id: &'a str,
    pub image_id: &'a str,
    pub url: &'a str,
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
                doc_id: "",
                image_id: id,
                url: src
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
// GDocs HTML renderer

// We need <'r> because the closure is boxed to be stored in the HtmlRenderer struct and we want
// to allow it to refer its scope mutably so that the caller can accumulate images referenced in
// a document and process them after conversion.
// Callbacks and lifetimes https://stackoverflow.com/questions/41081240/idiomatic-callbacks-in-rust


pub fn render <C: HtmlConsumer>(
    doc: &docs::Document,
    next: C,
    ) -> String {
    let mut renderer = HtmlRenderer {
        doc,
        html: String::new(),
        tags: Vec::new(),
        out: next,
    };

    renderer.format_doc();
    return renderer.html;
}

struct HtmlRenderer <'a, HC: HtmlConsumer> {
    // Input
    doc: &'a docs::Document,

    // State
    tags: Vec<&'a str>,

    // Output
    html: String,
    out: HC,
}

impl <HC: HtmlConsumer> AddAssign<&str> for HtmlRenderer<'_, HC> {
    fn add_assign(&mut self, rhs: &str) {
        self.add_html_text(rhs)
    }
}

impl <HC: HtmlConsumer> AddAssign<String> for HtmlRenderer<'_, HC> {
    fn add_assign(&mut self, rhs: String) {
        self.add_html_text(rhs.as_str());
    }
}

impl <HC: HtmlConsumer> AddAssign<&Option<String>> for HtmlRenderer<'_, HC> {
    fn add_assign(&mut self, rhs: &Option<String>) {
        if let Some(rhs) = rhs {
            self.add_html_text(rhs);
        }
    }
}

impl <HC: HtmlConsumer> AddAssign<&String> for HtmlRenderer<'_, HC> {
    fn add_assign(&mut self, rhs: &String) {
        self.add_html_text(rhs.as_str());
    }
}

#[derive(Default)]
struct Indent {
    depth: usize,
    magnitude: f64,
}

impl <'a, HC: HtmlConsumer> HtmlRenderer<'a, HC> {
    fn add_html_text(&mut self, text: &str) {
        self.html += text;
    }

    fn start_tag(&mut self, tag: &'a str) {
        self.tags.push(tag);
        *self += "<";
        *self += tag;
        *self += ">";
    }

    fn start_tag_class(&mut self, tag: &'a str, class: &str) {
        self.tags.push(tag);
        *self += "<";
        *self += tag;
        if class.len() > 0 {
            *self += " class='";
            *self += class;
            *self += "'";
        }
        *self += ">";
    }

    fn start_tag_style(&mut self, tag: &'a str, style: &str) {
        self.tags.push(tag);
        *self += "<";
        *self += tag;
        if style.len() > 0 {
            *self += " style='";
            *self += style;
            *self += "'";
        }
        *self += ">";
    }

    fn start_tag_class_style(&mut self, tag: &'a str, class: &str, style: &str) {
        self.tags.push(tag);
        *self += "<";
        *self += tag;
        if class.len() > 0 {
            *self += " class='";
            *self += class;
            *self += "'";
        }
        if style.len() > 0 {
            *self += " style='";
            *self += style;
            *self += "'";
        }
        *self += ">";
    }

    fn end_tag(&mut self) {
        let tag = self.tags.pop().unwrap();
        *self += "</";
        *self += tag;
        *self += ">";
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
        if text.ends_with('\n') {
            text = &text[.. text.len() - 1]
        }

        let mut split = text.split('\u{000B}');
        if let Some(first) = split.next() {
            *self += first;
            for next in split {
                *self += "<br>\n";
                *self += next;
            }
        }
    }

    /// Result should be escaped, and safe for inclusion in html
    fn convert_url(&self, url: impl AsRef<str>) -> String {
        // TODO
        url.as_ref().to_string()
    }

    fn format_doc(&mut self) {
        self.start_tag("html");
        self.nl();

        self.start_tag("head");
        self.nl();
        if let Some(title) = &self.doc.title {
            self.start_tag("title");
            *self += title;
            self.end_tag_nl();
        }
        self.end_tag_nl();
        self.start_tag("body");

        self.format_body();
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
    }

    /// The document body.
    fn format_body(&mut self) {
        if let Some(body) = &self.doc.body {
            self.format_structural_elements(&body.content);
        }
    }

    fn format_structural_elements(&mut self, elements: &'a Option<Vec<docs::StructuralElement>>) {
        let mut indent = Indent::default();
        if let Some(elements) = elements {
            for elt in elements {
                self.format_structural_element(elt, &mut indent);
            }
        }
        for _ in 0..indent.depth {
            self.end_tag_nl();
        }
    }

    fn format_structural_element(&mut self, elt: &'a docs::StructuralElement, indent: &mut Indent) {
        if let Some(para) = &elt.paragraph {
            self.format_paragraph(&para, indent);

        } else if let Some(table) = &elt.table {
            self.format_table(&table);

        } else if let Some(section_break) = &elt.section_break {
            self.format_section_break(section_break);

        } else if let Some(toc) = &elt.table_of_contents {
            self.format_table_of_contents(toc);

        } else {
            unimplemented!("Unknown structural element type {:?}", elt);
        }
    }

    fn format_table_of_contents(&mut self, toc: &'a docs::TableOfContents) {
        self.start_tag_class("div", "table-of-contents");
        self.format_structural_elements(&toc.content);
        self.end_tag_nl();
    }

    fn format_section_break(&mut self, section: &docs::SectionBreak) {
        // FIXME
        // Ignore for now
        // The interesting bits: column properties (multi-column sections)
        // Should create a <div> containing following sibling elements until the next section break
    }

    /// A paragraph is a range of content that is terminated with a newline character.
    fn format_paragraph(&mut self, para: &'a docs::Paragraph, indent: &mut Indent) {

        // Find this paragraph's nesting level and add/close <ul>s accordingly
        let cur_depth = indent.depth;
        let mut new_depth = cur_depth;

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
            self.start_tag("ul");
            self.nl();
        }

        for _ in new_depth..cur_depth {
            self.end_tag();
            self.nl();
        }

        indent.depth = new_depth;
        indent.magnitude = new_magnitude;

        // Ignored:
        // - para.suggested_paragraph_style_changes
        // - para.suggested_bullet_changes
        // - para.suggested_positioned_object_ids

        let mut tag = "p";
        let mut class: &'a str = "";

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
                style_attr += "text-align:";
                style_attr += match align.as_str() {
                    "START" => "start",
                    "END" => "end",
                    "CENTER" => "center",
                    "JUSTIFIED" => "justify",
                    _ => "inherit", // "UNSPECIFIED" or other value
                };
                style_attr += ";";
            }
        }

        if let Some(bullet) = &para.bullet {
            tag = "li";
        }

        self.nl();
        self.start_tag_class_style(tag, class, style_attr.as_str());

        if let Some(elements) = &para.elements {
            for elt in elements {
                self.format_paragraph_element(elt);
            }
        }

        self.end_tag_nl();
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

        } else if let Some(footnote_ref) = &elt.footnote_reference {

        } else if let Some(hr) = &elt.horizontal_rule {
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

        } else if let Some(link) = &elt.rich_link {
            // TODO
        } else {
            unimplemented!("Unknown paragraph element {:?}", elt);
        }
    }

    fn format_embedded_object(&mut self, id: &str, obj: &docs::EmbeddedObject) {
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

            self.start_tag_style("span", &span_style);

            *self += "<img src='";
            // add id attribute
            *self += img.content_uri.as_ref().unwrap();
            // let new_uri = &(self.image_resolver)(&ImageReference {
            //     doc_id: self.doc.document_id.as_ref().unwrap(),
            //     image_id: id,
            //     url: img.content_uri.as_ref().unwrap(),
            // });
            // *self += new_uri;
            *self += "' style='";
            *self += img_style;
            *self += "'>";

            self.end_tag();

        }
    }

    fn format_text_run(&mut self, text: &docs::TextRun) {
        let mut style_attr = String::new();
        let mut elts = Vec::<&str>::new();

        let mut link: Option<&String> = None;

        if let Some(style) = &text.text_style {
            if let Some(style_link) = &style.link {
                link = style_link.url.as_ref();
                // NOTE: Links have foreground-color and underline styles. We'll skip them
                // below to avoid interfering with the CSS style.
                // We may want to keep these however if they're the same as the ones of the
                // previous/next text runs (link in a styled paragraph)
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
        if let Some(url) = link {
            *self += "<a href='";
            *self +=  self.convert_url(url);
            *self += "'>";
        }

        // Start <del>, <sup> etc.
        for elt in &elts {
            self.start_tag(elt);
        }

        // Start <span> if we have custom styles. We do not add style on a surrounding tag, as it
        // may conflict with that tag's default styling (e.g. strike-through in <del> may be
        // overriden by an underlined style)
        if style_attr.len() > 0 {
            self.start_tag_style("span", &style_attr);
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

    fn format_table(&mut self, table: &'a docs::Table) {
        *self += "<table border>\n";

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
                        self.format_structural_elements(&cell.content);

                        *self += "</td>\n";
                    }
                }
                self.nl();
                *self += "</tr>\n";
            }
        }
        *self += "</table>\n";
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
                    "{}:rgb({},{},{});",
                    name,
                    color.red.unwrap(),
                    color.green.unwrap(),
                    color.blue.unwrap()
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

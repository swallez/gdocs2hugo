use html5ever::serialize::{HtmlSerializer, SerializeOpts};
use markup5ever::serialize::{AttrRef, Serialize, Serializer, TraversalScope};
use markup5ever::QualName;
use std::io;

/// A DOM serializer that produces a stable output.
///
/// Element attributes are stored in a `HashMap`, that uses a random seed to distribute value in
/// hash buckets, causing iteration order to also be random. This serializer first sorts attributes
/// by their local name before serializing them, thus guaranteeing a stable serialization.
///
struct StableHtmlSerializer<Wr: std::io::Write> {
    writer: Wr,
    opts: SerializeOpts,
}

impl <Wr: std::io::Write> StableHtmlSerializer<Wr> {
    pub fn new(writer: Wr, opts: SerializeOpts) -> Self {
        Self { writer, opts }
    }
}

impl<Wr: std::io::Write> Serializer for StableHtmlSerializer<Wr> {

    fn start_elem<'a, AttrIter>(&mut self, name: QualName, attrs: AttrIter) -> io::Result<()>
    where
        AttrIter: Iterator<Item = AttrRef<'a>>,
    {
        let mut attrs_vec = attrs.collect::<Vec<_>>();
        attrs_vec.sort_by(|(qname1, _), (qname2, _)| qname1.local.cmp(&qname2.local));

        let mut inner = HtmlSerializer::new(&mut self.writer, self.opts.clone());
        inner.start_elem(name, attrs_vec.into_iter())
    }

    fn end_elem(&mut self, name: QualName) -> io::Result<()> {
        let mut inner = HtmlSerializer::new(&mut self.writer, self.opts.clone());
        inner.end_elem(name)
    }

    fn write_text(&mut self, text: &str) -> io::Result<()> {
        let mut inner = HtmlSerializer::new(&mut self.writer, self.opts.clone());
        inner.write_text(text)
    }

    fn write_comment(&mut self, text: &str) -> io::Result<()> {
        if text.starts_with("{{<") {
            self.writer.write(text.as_bytes()).map(|_| ())?;
            self.writer.write(&[b'\n']).map(|_| ())
        } else {
            let mut inner = HtmlSerializer::new(&mut self.writer, self.opts.clone());
            inner.write_comment(text)
        }
    }

    fn write_doctype(&mut self, name: &str) -> io::Result<()> {
        let mut inner = HtmlSerializer::new(&mut self.writer, self.opts.clone());
        inner.write_doctype(name)
    }

    fn write_processing_instruction(&mut self, target: &str, data: &str) -> io::Result<()> {
        let mut inner = HtmlSerializer::new(&mut self.writer, self.opts.clone());
        inner.write_processing_instruction(target, data)
    }
}

pub fn stable_html(doc: &scraper::Html) -> anyhow::Result<String> {
    let opts = SerializeOpts {
        scripting_enabled: false, // It's not clear what this does.
        traversal_scope: TraversalScope::IncludeNode,
        create_missing_parent: false,
    };

    let mut buf = Vec::new();
    let mut ser = StableHtmlSerializer::new(&mut buf, opts);

    let root = doc.root_element();
    root.serialize( &mut ser, TraversalScope::IncludeNode)?;

    Ok(String::from_utf8(buf).unwrap())
}

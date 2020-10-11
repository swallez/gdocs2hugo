use html5ever::serialize::HtmlSerializer;
use html5ever::serialize::SerializeOpts;
use markup5ever::serialize::AttrRef;
use markup5ever::serialize::Serialize;
use markup5ever::serialize::Serializer;
use markup5ever::serialize::TraversalScope;
use markup5ever::QualName;
use scraper::element_ref::ElementRef;
use std::io;

/// A DOM serializer that produces a stable output.
///
/// Element attributes are stored in a `HashMap`, that uses a random seed to distribute value in
/// hash buckets, causing iteration order to also be random. This serializer first sorts attributes
/// by their local name before serializing them, thus guaranteeing a stable serialization.
///
struct StableHtmlSerializer<S: Serializer>(S);

impl<S: Serializer> Serializer for StableHtmlSerializer<S> {
    fn start_elem<'a, AttrIter>(&mut self, name: QualName, attrs: AttrIter) -> io::Result<()>
    where
        AttrIter: Iterator<Item = AttrRef<'a>>,
    {
        let mut attrs_vec = attrs.collect::<Vec<_>>();
        attrs_vec.sort_by(|(qname1, _), (qname2, _)| qname1.local.cmp(&qname2.local));

        self.0.start_elem(name, attrs_vec.into_iter())
    }

    fn end_elem(&mut self, name: QualName) -> io::Result<()> {
        self.0.end_elem(name)
    }

    fn write_text(&mut self, text: &str) -> io::Result<()> {
        self.0.write_text(text)
    }

    fn write_comment(&mut self, text: &str) -> io::Result<()> {
        self.0.write_comment(text)
    }

    fn write_doctype(&mut self, name: &str) -> io::Result<()> {
        self.0.write_doctype(name)
    }

    fn write_processing_instruction(&mut self, target: &str, data: &str) -> io::Result<()> {
        self.0.write_processing_instruction(target, data)
    }
}

pub fn stable_html(doc: &scraper::Html) -> anyhow::Result<String> {
    let opts = SerializeOpts {
        scripting_enabled: false, // It's not clear what this does.
        traversal_scope: TraversalScope::IncludeNode,
        create_missing_parent: false,
    };

    let mut buf = Vec::new();
    let mut ser = StableHtmlSerializer(HtmlSerializer::new(&mut buf, opts));

    let root = doc.root_element();
    // Need fully qualified call to disambiguate overloaded serialize() method
    <ElementRef as Serialize>::serialize(&root, &mut ser, TraversalScope::IncludeNode)?;

    Ok(String::from_utf8(buf).unwrap())
}

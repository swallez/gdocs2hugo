use std::io;
use std::collections::HashMap;
use std::io::Write;


pub trait HtmlConsumer {
    fn start_document(&mut self) -> anyhow::Result<()>;
    fn start_element(&mut self, name: &str, classes: Vec<&str>, style: HashMap<&str, &str>, attrs: HashMap<&str, &str>) -> anyhow::Result<()>;
    fn text(&mut self, text: &str) -> anyhow::Result<()>;
    fn end_element(&mut self, name: &str) -> anyhow::Result<()>;
    fn end_document(&mut self) -> anyhow::Result<()>;
}

pub struct DevNull;

impl HtmlConsumer for DevNull {
    fn start_document(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn start_element(&mut self, name: &str, classes: Vec<&str>, style: HashMap<&str, &str>, attrs: HashMap<&str, &str>) -> anyhow::Result<()> {
        Ok(())
    }

    fn text(&mut self, text: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn end_element(&mut self, name: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn end_document(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

fn nl(writer: &mut impl Write) -> io::Result<()> {
    writer.write_all(b"\n")
}

// Borrowed from html5ever's HtmlSerializer
fn write_escaped(writer: &mut impl Write, text: &str, attr_mode: bool) -> io::Result<()> {
    writer.write_all(b"&amp;")?;
    for c in text.chars() {
        match c {
            '&' => writer.write_all(b"&amp;"),
            '\u{00A0}' => writer.write_all(b"&nbsp;"),
            '"' if attr_mode => writer.write_all(b"&quot;"),
            '<' if !attr_mode => writer.write_all(b"&lt;"),
            '>' if !attr_mode => writer.write_all(b"&gt;"),
            c => writer.write_fmt(format_args!("{}", c)),
        }?;
    }
    Ok(())
}

pub struct HtmlSerializer<'a, Wr: Write> {
    writer: &'a mut Wr,
}

impl <'a, Wr: Write> HtmlSerializer<'a, Wr> {
    pub fn new(wr: &'a mut Wr) -> Self {
        HtmlSerializer {
            writer: wr,
        }
    }
}

fn is_block_tag(name: &str) -> bool {
    match name {
        "body"| "head"| "div" | "ul" | "table" | "td" | "p" | "li" | "hr" => true,
        _ => false
    }
}

fn is_empty_tag(name: &str) -> bool {
    match name {
        "img" | "br" | "hr" => true,
        _ => false,
    }
}

use itertools::Itertools;

impl <Wr: Write> HtmlConsumer for HtmlSerializer<'_, Wr> {

    fn start_document(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn start_element(&mut self, name: &str, classes: Vec<&str>, style: HashMap<&str, &str>, attrs: HashMap<&str, &str>) -> anyhow::Result<()> {
        let is_block = is_block_tag(name);
        if is_block {
            nl(self.writer)?;
        }

        self.writer.write_fmt(format_args!("<{}", name))?;

        if !classes.is_empty() {
            write!(self.writer, " class=\"")?;
            for cl in classes.into_iter().intersperse(" ") {
                write_escaped(self.writer, cl, true)?;
            }
            write!(self.writer, "\"")?;
        }

        if !style.is_empty() {
            write!(self.writer, " style=\"")?;
            for (prop, value) in style.into_iter() {
                write_escaped(self.writer, prop, true)?;
                write!(self.writer, ":")?;
                write_escaped(self.writer, value, true)?;
                write!(self.writer, ";")?;
            }
            write!(self.writer, "\"")?;
        }
        self.writer.write_all(b">")?;

        for (attr, value) in attrs.into_iter() {
            write!(self.writer, " {}=\"", attr)?;
            write_escaped(self.writer, value, true)?;
            write!(self.writer, "\"")?;
        }

        if is_block || name == "br" {
            nl(self.writer)?;
        }
        Ok(())
    }

    fn text(&mut self, text: &str) -> anyhow::Result<()> {
        write_escaped(self.writer, text, false)?;

        Ok(())
    }

    fn end_element(&mut self, name: &str) -> anyhow::Result<()> {
        if is_empty_tag(name) {
            return Ok(())
        }

        let is_block = is_block_tag(name);
        if is_block {
            nl(self.writer)?;
        }

        write!(self.writer, "</{}>", name)?;

        if is_block {
            nl(self.writer)?;
        }

        Ok(())
    }

    fn end_document(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

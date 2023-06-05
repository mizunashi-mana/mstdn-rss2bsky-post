use std::error::Error;

pub type RichText = Vec<RichTextSegment>;

pub enum RichTextSegment {
    PlainText { text: String },
    Link { text: String, link: String },
}

mod from_html_impl;

pub fn from_html(content: &str) -> Result<RichText, Box<dyn Error>> {
    from_html_impl::from_html(content)
}

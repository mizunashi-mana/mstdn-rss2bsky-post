use html5ever::tokenizer::{TokenSink, TokenSinkResult, Tokenizer, BufferQueue, Token, TagKind, Tag};
use html5ever::tendril::{SliceExt};
use std::error::Error;

use crate::richtext::{RichText, RichTextSegment};

struct Html2RichTextSink {
    text: RichText,
    tag_depth: usize,
    state: ProcessState,
    err: Option<String>,
}

enum ProcessState {
    NotProcessed,
    ProcessingPlainText {
        text_continue: String,
    },
    ProcessingLink {
        link_tag_depth: usize,
        link: String,
        text_continue: String,
    },
}

impl Html2RichTextSink {
    fn process_plain_char(&mut self, c: char) -> () {
        match &mut self.state {
            ProcessState::NotProcessed => {
                self.state = ProcessState::ProcessingPlainText {
                    text_continue: String::from(c),
                };
            }
            ProcessState::ProcessingPlainText { text_continue } => {
                text_continue.push(c);
            }
            ProcessState::ProcessingLink { text_continue, .. } => {
                text_continue.push(c);
            }
        }
    }

    fn process_start_link(&mut self, tag: &Tag) -> () {
        let mut link_opt: Option<String> = None;
        for attr in &tag.attrs {
            match attr.name.local.to_string().as_str() {
                "href" => {
                    link_opt = Some(attr.value.to_string());
                }
                _ => {
                    // do nothing
                }
            }
        }

        match link_opt {
            None => {
                // do nothing
            }
            Some(link) => {
                match self.state {
                    ProcessState::NotProcessed | ProcessState::ProcessingPlainText { .. } => {
                        self.end_process();
                        self.state = ProcessState::ProcessingLink {
                            link,
                            link_tag_depth: self.tag_depth,
                            text_continue: String::from(""),
                        };
                    }
                    ProcessState::ProcessingLink { .. } => {
                        // do nothing
                    }
                }
            }
        }
    }

    fn process_start_tag(&mut self, tag: &Tag) -> () {
        match tag.name.to_string().as_str() {
            "br" => {
                self.process_plain_char('\n');
            }
            "a" => {
                self.process_start_link(&tag);
            }
            _ => {
                // do nothing
            }
        }
        self.tag_depth += 1;
    }

    fn process_eng_tag(&mut self, tag: &Tag) -> () {
        self.tag_depth -= 1;
        match tag.name.to_string().as_str() {
            "a" => {
                self.end_process();
            }
            _ => {
                // do nothing
            }
        }
    }

    fn end_process(&mut self) -> () {
        match &self.state {
            ProcessState::NotProcessed => {
                // do nothing
            }
            ProcessState::ProcessingPlainText { text_continue } => {
                self.text.push(RichTextSegment::PlainText {
                    text: text_continue.to_string(),
                });
            }
            ProcessState::ProcessingLink { text_continue, link, link_tag_depth } => {
                if self.tag_depth <= *link_tag_depth {
                    self.text.push(RichTextSegment::Link {
                        text: text_continue.to_string(),
                        link: link.to_string(),
                    });
                }
            }
        }
        self.state = ProcessState::NotProcessed;
    }
}

impl TokenSink for Html2RichTextSink {
    type Handle = ();

    fn process_token(&mut self, token: Token, _line_number: u64) -> TokenSinkResult<Self::Handle> {
        match token {
            Token::CharacterTokens(bs) => {
                for c in bs.chars() {
                    self.process_plain_char(c);
                }
            }
            Token::NullCharacterToken => {
                self.process_plain_char('\0');
            }
            Token::TagToken(tag) => {
                match tag.kind {
                    TagKind::StartTag => {
                        self.process_start_tag(&tag);
                        if tag.self_closing {
                            self.process_eng_tag(&tag);
                        }
                    }
                    TagKind::EndTag => {
                        self.process_eng_tag(&tag);
                    }
                }
            }
            Token::DoctypeToken(_) | Token::CommentToken(_) => {
                // do nothing
            }
            Token::EOFToken => {
                self.end_process();
            }
            Token::ParseError(err) => {
                self.err = Some(String::from(err));
            }
        }
        TokenSinkResult::Continue
    }
}

pub fn from_html(content: &str) -> Result<RichText, Box<dyn Error>> {
    let mut tokenizer = Tokenizer::new(
        Html2RichTextSink {
            text: vec![],
            tag_depth: 0,
            state: ProcessState::NotProcessed,
            err: None,
        },
        Default::default(),
    );

    let mut queue = BufferQueue::new();
    queue.push_back(content.to_tendril());

    let _ = tokenizer.feed(&mut queue);
    tokenizer.end();

    match tokenizer.sink.err {
        Some(err) => {
            Err(Box::<dyn Error>::from(err))?
        }
        None => {
            Ok(tokenizer.sink.text)
        }
    }
}

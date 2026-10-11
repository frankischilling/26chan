use super::*;
use crate::wordfilter::LeetRolls;

impl FrozenPresentation {
    pub fn encode(&self) -> Result<Vec<u8>> {
        let sizes = self.validate()?;
        let mut output = Vec::with_capacity(sizes.encoded);
        output.extend_from_slice(match self.bounds {
            WordfilterLimits::Ordinary => b"FP01",
            WordfilterLimits::Authorized => b"FP02",
        });
        output.push(u8::from(self.wordfiltered));
        put_u32(&mut output, self.atoms.len());
        for atom in &self.atoms {
            match atom {
                Atom::Text(text) => {
                    output.push(0);
                    put_text(&mut output, text);
                }
                Atom::LineBreak => output.push(1),
                Atom::WordBreak => output.push(2),
                Atom::Markup(tag, open) => {
                    output.extend_from_slice(&[3, tag_number(*tag), u8::from(*open)])
                }
                Atom::QuoteSpan(open) => output.extend_from_slice(&[4, u8::from(*open)]),
                Atom::FilteredDelimiter(value) => {
                    output.extend_from_slice(&[
                        5,
                        tag_number(value.tag()),
                        u8::from(value.opening()),
                    ]);
                    output.extend_from_slice(
                        &value
                            .rolls()
                            .ok_or(PresentationError::InvalidEncoding)?
                            .choices(),
                    );
                }
                Atom::ChangedEntity(value) => output.extend_from_slice(&[
                    6,
                    match value {
                        ChangedEntity::Amp => 0,
                        ChangedEntity::QuoteZero => 1,
                        ChangedEntity::QuoteSeven => 2,
                        ChangedEntity::QuoteZeroSeven => 3,
                    },
                ]),
                Atom::StaticLink(quote, parts) => {
                    output.push(7);
                    put_text(&mut output, &quote.label());
                    put_parts(&mut output, parts);
                }
                Atom::ServerLink(link, parts) => {
                    output.push(8);
                    put_text(&mut output, link.href());
                    put_parts(&mut output, parts);
                }
            }
        }
        debug_assert_eq!(output.len(), sizes.encoded);
        Ok(output)
    }

    /// Decode only the requested authority class. A valid FP02 payload cannot
    /// expand an ordinary row's limits. Every rendering component is validated.
    pub fn decode(bytes: &[u8], expected_limits: PostLimits) -> Result<Self> {
        let bounds = WordfilterLimits::for_post(expected_limits);
        let magic = match bounds {
            WordfilterLimits::Ordinary => b"FP01",
            WordfilterLimits::Authorized => b"FP02",
        };
        if bytes.len() < 9 || bytes.len() > bounds.stored_bytes() || !bytes.starts_with(magic) {
            return Err(PresentationError::InvalidEncoding);
        }
        let mut cursor = Cursor {
            bytes,
            offset: 4,
            units: 0,
            max_units: bounds.max_parts(),
        };
        let wordfiltered = cursor.boolean()?;
        let count = cursor.count()?;
        let mut atoms = Vec::with_capacity(count);
        for _ in 0..count {
            atoms.push(match cursor.byte()? {
                0 => Atom::Text(cursor.text()?.into()),
                1 => Atom::LineBreak,
                2 => Atom::WordBreak,
                3 => Atom::Markup(tag_from(cursor.byte()?)?, cursor.boolean()?),
                4 => Atom::QuoteSpan(cursor.boolean()?),
                5 => {
                    let tag = tag_from(cursor.byte()?)?;
                    let open = cursor.boolean()?;
                    let rolls = LeetRolls::from_choices(cursor.byte()?, cursor.byte()?)
                        .map_err(|_| PresentationError::InvalidEncoding)?;
                    Atom::FilteredDelimiter(Delimiter::from_saved(tag, open, Some(rolls)))
                }
                6 => Atom::ChangedEntity(match cursor.byte()? {
                    0 => ChangedEntity::Amp,
                    1 => ChangedEntity::QuoteZero,
                    2 => ChangedEntity::QuoteSeven,
                    3 => ChangedEntity::QuoteZeroSeven,
                    _ => return Err(PresentationError::InvalidEncoding),
                }),
                7 => {
                    let text = cursor.text()?;
                    let (quote, used) =
                        StaticQuote::parse(text).ok_or(PresentationError::InvalidEncoding)?;
                    if used != text.len() {
                        return Err(PresentationError::InvalidEncoding);
                    }
                    Atom::StaticLink(quote, cursor.parts()?)
                }
                8 => {
                    let text = cursor.text()?;
                    let (link, used) =
                        ServerLink::parse(text).ok_or(PresentationError::InvalidEncoding)?;
                    if used != text.len() {
                        return Err(PresentationError::InvalidEncoding);
                    }
                    Atom::ServerLink(link, cursor.parts()?)
                }
                _ => return Err(PresentationError::InvalidEncoding),
            });
        }
        if cursor.offset != bytes.len() {
            return Err(PresentationError::InvalidEncoding);
        }
        let result = Self {
            atoms,
            bounds,
            wordfiltered,
        };
        result.validate()?;
        Ok(result)
    }

    pub(super) fn validate(&self) -> Result<Sizes> {
        let mut sizes = Sizes {
            encoded: 9,
            source: 0,
            search: 0,
        };
        let mut units = self.atoms.len();
        if units > self.bounds.max_parts() {
            return Err(PresentationError::LimitExceeded);
        }
        let mut stacks: [Vec<Option<LeetRolls>>; 8] = std::array::from_fn(|_| Vec::new());
        let mut quote_open = false;
        let mut previous_text = false;
        for atom in &self.atoms {
            let mut delimiter = None;
            let (encoded, source, search) = match atom {
                Atom::Text(text) => {
                    if text.is_empty() || previous_text {
                        return Err(PresentationError::InvalidEncoding);
                    }
                    (5 + text.len(), escaped_len(text), text.len())
                }
                Atom::LineBreak => (1, 4, 1),
                Atom::WordBreak => (1, 5, 0),
                Atom::Markup(tag, open) => {
                    delimiter = Some((*tag, *open, None));
                    (3, crate::robot9000::markup(*tag, *open).len(), 0)
                }
                Atom::QuoteSpan(open) => {
                    if quote_open == *open {
                        return Err(PresentationError::InvalidEncoding);
                    }
                    quote_open = *open;
                    (
                        2,
                        if *open {
                            "<span class=\"quote\">".len()
                        } else {
                            "</span>".len()
                        },
                        0,
                    )
                }
                Atom::FilteredDelimiter(value) => {
                    if !self.wordfiltered || value.rolls().is_none() {
                        return Err(PresentationError::InvalidEncoding);
                    }
                    delimiter = Some((value.tag(), value.opening(), value.rolls()));
                    let source = value.source_projection().len();
                    (
                        5,
                        source,
                        if !value.valid_element() && value.opening() {
                            source
                        } else {
                            0
                        },
                    )
                }
                Atom::ChangedEntity(value) => {
                    if !self.wordfiltered {
                        return Err(PresentationError::InvalidEncoding);
                    }
                    (2, value.spelling().len(), value.spelling().len())
                }
                Atom::StaticLink(quote, parts) => {
                    let label = quote.label();
                    let (parsed, consumed) =
                        StaticQuote::parse(&label).ok_or(PresentationError::InvalidEncoding)?;
                    if consumed != label.len() || parsed != *quote {
                        return Err(PresentationError::InvalidEncoding);
                    }
                    let part_sizes = validate_parts(parts, &label)?;
                    units = units
                        .checked_add(parts.len())
                        .ok_or(PresentationError::LimitExceeded)?;
                    (
                        1 + 4 + label.len() + part_sizes.encoded,
                        "<a href=\"".len()
                            + escaped_len(&quote.source_href())
                            + "\" class=\"quotelink\"".len()
                            + if quote.opens_new_tab() {
                                " target=\"_blank\"".len()
                            } else {
                                0
                            }
                            + 1
                            + part_sizes.source
                            + "</a>".len(),
                        part_sizes.search,
                    )
                }
                Atom::ServerLink(link, parts) => {
                    let (parsed, consumed) =
                        ServerLink::parse(link.href()).ok_or(PresentationError::InvalidEncoding)?;
                    if consumed != link.href().len() || parsed != *link {
                        return Err(PresentationError::InvalidEncoding);
                    }
                    let part_sizes = validate_parts(parts, link.href())?;
                    units = units
                        .checked_add(parts.len())
                        .ok_or(PresentationError::LimitExceeded)?;
                    (
                        1 + 4 + link.href().len() + part_sizes.encoded,
                        "<a href=\"".len()
                            + escaped_len(link.href())
                            + "\" target=\"_blank\">".len()
                            + part_sizes.source
                            + "</a>".len(),
                        part_sizes.search,
                    )
                }
            };
            previous_text = matches!(atom, Atom::Text(_));
            if let Some((tag, open, rolls)) = delimiter {
                let stack = &mut stacks[tag_number(tag) as usize];
                if open {
                    let max_depth = if matches!(tag, Tag::Spoiler | Tag::Code) {
                        2
                    } else {
                        1
                    };
                    if stack.len() >= max_depth {
                        return Err(PresentationError::InvalidEncoding);
                    }
                    stack.push(rolls);
                } else if stack.pop() != Some(rolls) {
                    return Err(PresentationError::InvalidEncoding);
                }
            }
            for (total, amount) in [
                (&mut sizes.encoded, encoded),
                (&mut sizes.source, source),
                (&mut sizes.search, search),
            ] {
                *total = total
                    .checked_add(amount)
                    .ok_or(PresentationError::LimitExceeded)?;
            }
            if units > self.bounds.max_parts() {
                return Err(PresentationError::LimitExceeded);
            }
            sizes.check(self.bounds)?;
        }
        if quote_open || stacks.iter().any(|stack| !stack.is_empty()) {
            return Err(PresentationError::InvalidEncoding);
        }
        Ok(sizes)
    }
}

fn validate_parts(parts: &[WordPart], expected: &str) -> Result<Sizes> {
    let actual = parts.iter().flat_map(|part| match part {
        WordPart::Text(text) => text.as_bytes(),
        WordPart::Break => &[],
    });
    if !actual.copied().eq(expected.bytes()) {
        return Err(PresentationError::InvalidEncoding);
    }
    let mut sizes = Sizes {
        encoded: 4,
        source: 0,
        search: 0,
    };
    let mut previous_text = false;
    for part in parts {
        match part {
            WordPart::Text(text) => {
                if text.is_empty() || previous_text {
                    return Err(PresentationError::InvalidEncoding);
                }
                sizes.encoded += 5 + text.len();
                sizes.source += escaped_len(text);
                sizes.search += text.len();
                previous_text = true;
            }
            WordPart::Break => {
                sizes.encoded += 1;
                sizes.source += 5;
                previous_text = false;
            }
        }
    }
    Ok(sizes)
}

fn put_u32(output: &mut Vec<u8>, value: usize) {
    output.extend_from_slice(&(value as u32).to_be_bytes());
}
fn put_text(output: &mut Vec<u8>, text: &str) {
    put_u32(output, text.len());
    output.extend_from_slice(text.as_bytes());
}
fn put_parts(output: &mut Vec<u8>, parts: &[WordPart]) {
    put_u32(output, parts.len());
    for part in parts {
        match part {
            WordPart::Text(text) => {
                output.push(0);
                put_text(output, text);
            }
            WordPart::Break => output.push(1),
        }
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
    units: usize,
    max_units: usize,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(PresentationError::InvalidEncoding)?;
        let result = self
            .bytes
            .get(self.offset..end)
            .ok_or(PresentationError::InvalidEncoding)?;
        self.offset = end;
        Ok(result)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn boolean(&mut self) -> Result<bool> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(PresentationError::InvalidEncoding),
        }
    }
    fn number(&mut self) -> Result<usize> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| PresentationError::InvalidEncoding)?,
        ) as usize)
    }
    fn count(&mut self) -> Result<usize> {
        let count = self.number()?;
        self.units = self
            .units
            .checked_add(count)
            .ok_or(PresentationError::InvalidEncoding)?;
        if self.units > self.max_units || count > self.bytes.len() - self.offset {
            return Err(PresentationError::InvalidEncoding);
        }
        Ok(count)
    }
    fn text(&mut self) -> Result<&'a str> {
        let length = self.number()?;
        std::str::from_utf8(self.take(length)?).map_err(|_| PresentationError::InvalidEncoding)
    }
    fn parts(&mut self) -> Result<Vec<WordPart>> {
        let count = self.count()?;
        let mut parts = Vec::with_capacity(count);
        for _ in 0..count {
            parts.push(match self.byte()? {
                0 => WordPart::Text(self.text()?.into()),
                1 => WordPart::Break,
                _ => return Err(PresentationError::InvalidEncoding),
            });
        }
        Ok(parts)
    }
}

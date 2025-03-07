use std::ops::{Bound, RangeBounds};
use unicode_normalization_alignments::UnicodeNormalization;

/// Represents a Range usable by the NormalizedString to index its content.
/// A Range can use indices relative to either the `Original` or the `Normalized` string
#[derive(Debug, Clone)]
pub enum Range<T: RangeBounds<usize> + Clone> {
    Original(T),
    Normalized(T),
}

impl<T> Range<T>
where
    T: RangeBounds<usize> + Clone,
{
    /// Unwrap the underlying range
    fn unwrap(self) -> T {
        match self {
            Range::Original(r) => r,
            Range::Normalized(r) => r,
        }
    }

    /// Converts the current Range to a `std::ops::Range<usize>`. This requires the `max_len`
    /// of the represented string (in chars, not bytes) in order to cover the case where the
    /// original provided range was unbounded
    fn into_full_range(self, max_len: usize) -> std::ops::Range<usize> {
        let range = self.unwrap();

        let start = match range.start_bound() {
            Bound::Unbounded => 0,
            Bound::Included(i) => *i,
            Bound::Excluded(i) => *i + 1,
        };
        let end = match range.end_bound() {
            Bound::Unbounded => max_len,
            Bound::Included(i) => *i + 1,
            Bound::Excluded(i) => *i,
        };

        start..end
    }
}

/// A `NormalizedString` takes care of processing an "original" string to modify it and obtain a
/// "normalized" string. It keeps both version of the string, alignments information between both
/// and provides an interface to retrieve ranges of each string, using offsets from any of them.
///
/// It is possible to retrieve a part of the original string, by indexing it with offsets from the
/// normalized one, and the other way around too. It is also possible to convert offsets from one
/// referential to the other one easily.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct NormalizedString {
    /// The original version of the string, before any modification
    original: String,
    /// The normalized version of the string, after all modifications
    normalized: String,
    /// Mapping from normalized string to original one: (start, end) for each character of the
    /// normalized string
    alignments: Vec<(usize, usize)>,
}

impl NormalizedString {
    /// Create a NormalizedString from the given str
    pub fn from(s: &str) -> Self {
        NormalizedString {
            original: s.to_owned(),
            normalized: s.to_owned(),
            alignments: (0..s.chars().count()).map(|v| (v, v + 1)).collect(),
        }
    }

    /// Return the normalized string
    pub fn get(&self) -> &str {
        &self.normalized
    }

    /// Return the original string
    pub fn get_original(&self) -> &str {
        &self.original
    }

    /// Convert the given offsets range from one referential to the other one:
    /// `Original => Normalized` or `Normalized => Original`
    pub fn convert_offsets<T>(&self, range: Range<T>) -> Option<std::ops::Range<usize>>
    where
        T: RangeBounds<usize> + Clone,
    {
        match range {
            Range::Original(_) => {
                let (mut start, mut end) = (0, 0);
                let r = range.into_full_range(self.alignments.last().map_or(0, |(_, e)| *e));
                self.alignments
                    .iter()
                    .enumerate()
                    .take_while(|(_, alignment)| r.end >= alignment.1)
                    .for_each(|(i, alignment)| {
                        if alignment.0 <= r.start {
                            start = i;
                        }
                        if alignment.1 <= r.end {
                            end = i + 1;
                        }
                    });
                Some(start..end)
            }
            Range::Normalized(_) => self
                .alignments
                .get(range.into_full_range(self.alignments.len()))
                .map(|alignments| {
                    if alignments.is_empty() {
                        None
                    } else {
                        let start = alignments[0].0;
                        let end = alignments[alignments.len() - 1].1;
                        Some(start..end)
                    }
                })
                .flatten(),
        }
    }

    /// Return a range of the normalized string (indexing on char not bytes)
    pub fn get_range<T>(&self, range: Range<T>) -> Option<&str>
    where
        T: RangeBounds<usize> + Clone,
    {
        match range {
            Range::Original(_) => self
                .convert_offsets(range)
                .map(|r| get_range_of(&self.normalized, r))
                .flatten(),
            Range::Normalized(r) => get_range_of(&self.normalized, r),
        }
    }

    /// Return a range of the original string (indexing on char not bytes)
    pub fn get_range_original<T>(&self, range: Range<T>) -> Option<&str>
    where
        T: RangeBounds<usize> + Clone,
    {
        match range {
            Range::Original(r) => get_range_of(&self.original, r),
            Range::Normalized(_) => self
                .convert_offsets(range)
                .map(|r| get_range_of(&self.original, r))
                .flatten(),
        }
    }

    /// Return a new NormalizedString that contains only the specified range, indexing on bytes
    pub fn slice_bytes<T>(&self, range: Range<T>) -> Option<NormalizedString>
    where
        T: RangeBounds<usize> + Clone,
    {
        let (r, s) = match range {
            Range::Original(_) => (
                range.clone().into_full_range(self.original.len()),
                &self.original,
            ),
            Range::Normalized(_) => (
                range.clone().into_full_range(self.normalized.len()),
                &self.normalized,
            ),
        };

        let (mut start, mut end) = (None, None);
        s.char_indices()
            .enumerate()
            .take_while(|(_, (b, _))| *b < r.end)
            .filter(|(_, (b, _))| *b >= r.start)
            .for_each(|(i, (b, c))| {
                if b == r.start {
                    start = Some(i);
                }
                if b + c.len_utf8() == r.end {
                    end = Some(i + 1);
                }
            });

        match range {
            Range::Original(_) => self.slice(Range::Original(start?..end?)),
            Range::Normalized(_) => self.slice(Range::Normalized(start?..end?)),
        }
    }

    /// Return a new NormalizedString that contains only the specified range, indexing on char
    pub fn slice<T>(&self, range: Range<T>) -> Option<NormalizedString>
    where
        T: RangeBounds<usize> + Clone,
    {
        let r_original = match range {
            Range::Original(_) => range.clone().into_full_range(self.len_original()),
            Range::Normalized(_) => self.convert_offsets(range.clone())?,
        };
        let r_normalized = match range {
            Range::Original(_) => self.convert_offsets(range)?,
            Range::Normalized(_) => range.into_full_range(self.len()),
        };

        // We need to shift the alignments according to the part of the original string that we
        // keep
        let alignment_shift = r_original.start;

        Some(Self {
            original: get_range_of(&self.original, r_original)?.to_owned(),
            normalized: get_range_of(&self.normalized, r_normalized.clone())?.to_owned(),
            alignments: self
                .alignments
                .get(r_normalized)?
                .to_vec()
                .iter()
                .map(|(start, end)| (start - alignment_shift, end - alignment_shift))
                .collect(),
        })
    }

    /// Applies transformations to the current normalized version, updating the current
    /// alignments with the new ones.
    /// This method expect an Iterator yielding each char of the new normalized string
    /// with a `change` isize equals to:
    ///   - `1` if this is a new char
    ///   - `-N` if the char is right before N removed chars
    ///   - `0` if this char represents the old one (even if changed)
    /// Since it is possible that the normalized string doesn't include some of the characters at
    /// the beginning of the original one, we need an `initial_offset` which represents the number
    /// of removed chars at the very beginning.
    ///
    /// `change` should never be more than `1`. If multiple chars are added, each of
    /// them has a `change` of `1`, but more doesn't make any sense.
    /// We treat any value above `1` as `1`.
    pub fn transform<I: Iterator<Item = (char, isize)>>(&mut self, dest: I, initial_offset: usize) {
        let mut offset = -(initial_offset as isize);
        let (normalized, alignments): (String, Vec<_>) = dest
            .enumerate()
            .map(|(index, (c, changes))| {
                // A positive offset means we added characters. So we need to remove this offset
                // from the current index to find out the previous id
                let idx = (index as isize - offset) as usize;
                offset += changes;
                let align = if changes.is_positive() {
                    if idx < 1 {
                        (0, 0)
                    } else {
                        // This is a newly inserted character, so we use the alignment from the
                        // previous one
                        self.alignments[idx - 1]
                    }
                } else {
                    self.alignments[idx]
                };
                // Then we keep only the char for string reconstruction
                (c, align)
            })
            .unzip();
        self.alignments = alignments;
        self.normalized = normalized;
    }

    /// Applies NFD normalization
    pub fn nfd(&mut self) -> &mut Self {
        self.transform(self.get().to_owned().nfd(), 0);
        self
    }

    /// Applies NFKD normalization
    pub fn nfkd(&mut self) -> &mut Self {
        self.transform(self.get().to_owned().nfkd(), 0);
        self
    }

    /// Applies NFC normalization
    pub fn nfc(&mut self) -> &mut Self {
        self.transform(self.get().to_owned().nfc(), 0);
        self
    }

    /// Applies NFKC normalization
    pub fn nfkc(&mut self) -> &mut Self {
        self.transform(self.get().to_owned().nfkc(), 0);
        self
    }

    /// Applies filtering over our characters
    pub fn filter<F: Fn(char) -> bool>(&mut self, keep: F) -> &mut Self {
        let mut removed = 0;
        let filtered = self
            .normalized
            .chars()
            .rev()
            .map(|c| {
                if keep(c) {
                    if removed > 0 {
                        let res = (c, -(removed as isize));
                        removed = 0;
                        Some(res)
                    } else {
                        Some((c, 0))
                    }
                } else {
                    removed += 1;
                    None
                }
            })
            .collect::<Vec<_>>();
        self.transform(filtered.into_iter().rev().filter_map(|o| o), removed);
        self
    }

    /// Prepend the given string to ourself
    pub fn prepend(&mut self, s: &str) -> &mut Self {
        self.normalized.insert_str(0, s);
        #[allow(clippy::reversed_empty_ranges)]
        self.alignments.splice(0..0, s.chars().map(|_| (0, 0)));
        self
    }

    /// Append the given string to ourself
    pub fn append(&mut self, s: &str) -> &mut Self {
        self.normalized.push_str(s);
        let last_offset = self.alignments.last().map_or((0, 0), |o| (o.1, o.1));
        self.alignments.extend(s.chars().map(|_| last_offset));
        self
    }

    /// Map our characters
    pub fn map<F: Fn(char) -> char>(&mut self, map: F) -> &mut Self {
        self.normalized = self.normalized.chars().map(map).collect::<String>();
        self
    }

    /// Calls the given function for each characters
    pub fn for_each<F: FnMut(char)>(&mut self, foreach: F) -> &mut Self {
        self.normalized.chars().for_each(foreach);
        self
    }

    /// Lowercase
    pub fn lowercase(&mut self) -> &mut Self {
        let mut new_chars: Vec<(char, isize)> = vec![];
        self.for_each(|c| {
            c.to_lowercase().enumerate().for_each(|(index, c)| {
                new_chars.push((c, if index > 0 { 1 } else { 0 }));
            })
        });
        self.transform(new_chars.into_iter(), 0);
        self
    }

    /// Uppercase
    pub fn uppercase(&mut self) -> &mut Self {
        let mut new_chars: Vec<(char, isize)> = vec![];
        self.for_each(|c| {
            c.to_uppercase().enumerate().for_each(|(index, c)| {
                new_chars.push((c, if index > 0 { 1 } else { 0 }));
            })
        });
        self.transform(new_chars.into_iter(), 0);
        self
    }

    /// Split off ourselves, returning a new Self that contains the range [at, len).
    /// self will then contain the range [0, at).
    /// The provided `at` indexes on `char` not bytes.
    pub fn split_off(&mut self, at: usize) -> Self {
        if at > self.len() {
            return NormalizedString::from("");
        }

        // Split normalized
        let byte_index = self.normalized.chars().enumerate().fold(0, |acc, (i, c)| {
            if i < at {
                acc + c.len_utf8()
            } else {
                acc
            }
        });
        let normalized = self.normalized.split_off(byte_index);
        let alignments = self.alignments.split_off(at);

        // Split original
        let original_at = self.alignments.last().map(|(_, end)| *end).unwrap_or(0);
        let original_byte_index = self.original.chars().enumerate().fold(0, |acc, (i, c)| {
            if i < original_at {
                acc + c.len_utf8()
            } else {
                acc
            }
        });
        let original = self.original.split_off(original_byte_index);

        NormalizedString {
            original,
            normalized,
            alignments,
        }
    }

    /// Merge with the given NormalizedString by appending it to self
    pub fn merge_with(&mut self, other: &NormalizedString) {
        self.original.push_str(&other.original);
        let len = self.len() - 1;
        self.alignments.extend(
            other
                .alignments
                .iter()
                .map(|(start, end)| (start + len, end + len)),
        );
        self.normalized.push_str(&other.normalized);
    }

    /// Remove any leading space(s) of the normalized string
    pub fn lstrip(&mut self) -> &mut Self {
        self.lrstrip(true, false)
    }

    /// Remove any trailing space(s) of the normalized string
    pub fn rstrip(&mut self) -> &mut Self {
        self.lrstrip(false, true)
    }

    /// Remove any leading and trailing space(s) of the normalized string
    pub fn strip(&mut self) -> &mut Self {
        self.lrstrip(true, true)
    }

    fn lrstrip(&mut self, left: bool, right: bool) -> &mut Self {
        let leading_spaces = if left {
            self.get().chars().take_while(|c| c.is_whitespace()).count()
        } else {
            0
        };
        let trailing_spaces = if right {
            self.get()
                .chars()
                .rev()
                .take_while(|c| c.is_whitespace())
                .count()
        } else {
            0
        };

        if leading_spaces > 0 || trailing_spaces > 0 {
            let transformation = self
                .normalized
                .chars()
                .enumerate()
                .filter_map(|(i, c)| {
                    if i < leading_spaces || i >= self.len() - trailing_spaces {
                        None
                    } else if i == self.len() - trailing_spaces - 1 {
                        Some((c, -(trailing_spaces as isize)))
                    } else {
                        Some((c, 0))
                    }
                })
                .collect::<Vec<_>>();
            self.transform(transformation.into_iter(), leading_spaces);
        }
        self
    }

    /// Returns the length of the normalized string (counting chars not bytes)
    pub fn len(&self) -> usize {
        self.normalized.chars().count()
    }

    /// Returns the length of the original string (counting chars not bytes)
    pub fn len_original(&self) -> usize {
        self.original.chars().count()
    }

    /// Whether empty
    pub fn is_empty(&self) -> bool {
        self.normalized.len() == 0
    }
}

/// Returns a range of the given string slice, by indexing chars instead of bytes
pub fn get_range_of<T: RangeBounds<usize>>(s: &str, range: T) -> Option<&str> {
    let len = s.chars().count();
    let start = match range.start_bound() {
        Bound::Unbounded => 0,
        Bound::Included(i) => *i,
        Bound::Excluded(i) => *i + 1,
    };
    let end = match range.end_bound() {
        Bound::Unbounded => len,
        Bound::Included(i) => *i + 1,
        Bound::Excluded(i) => *i,
    };

    if start >= len || end > len || start >= end {
        None
    } else {
        let start_b = s
            .char_indices()
            .map(|(i, _)| i)
            .nth(start as usize)
            .unwrap_or(0);
        let end_b = s
            .char_indices()
            .map(|(i, _)| i)
            .nth(end as usize)
            .unwrap_or_else(|| s.len());
        Some(&s[start_b..end_b])
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::reversed_empty_ranges)]
    use super::*;
    use unicode_categories::UnicodeCategories;

    #[test]
    fn new_chars() {
        let mut n = NormalizedString::from("élégant");
        n.nfd();
        assert_eq!(
            &n.alignments,
            &[
                (0, 1),
                (0, 1),
                (1, 2),
                (2, 3),
                (2, 3),
                (3, 4),
                (4, 5),
                (5, 6),
                (6, 7)
            ]
        );
    }

    #[test]
    fn unchanged() {
        let mut n = NormalizedString::from("élégant");
        n.nfd().filter(|c| !c.is_mark_nonspacing());
        assert_eq!(
            &n.alignments,
            &[(0, 1), (1, 2), (2, 3), (3, 4), (4, 5), (5, 6), (6, 7)]
        );
    }

    #[test]
    fn removed_chars() {
        let mut n = NormalizedString::from("élégant");
        n.filter(|c| c != 'n');
        assert_eq!(
            &n.alignments,
            &[(0, 1), (1, 2), (2, 3), (3, 4), (4, 5), (6, 7)]
        );
    }

    #[test]
    fn mixed_addition_and_removal() {
        let mut n = NormalizedString::from("élégant");
        n.nfd().filter(|c| !c.is_mark_nonspacing() && c != 'n');
        assert_eq!(
            &n.alignments,
            &[(0, 1), (1, 2), (2, 3), (3, 4), (4, 5), (6, 7)]
        );
    }

    #[test]
    fn range_conversion() {
        let mut n = NormalizedString::from("    __Hello__   ");
        n.filter(|c| !c.is_whitespace()).lowercase();
        let hello_n = n.convert_offsets(Range::Original(6..11));
        assert_eq!(hello_n, Some(2..7));
        assert_eq!(
            n.get_range(Range::Normalized(hello_n.clone().unwrap())),
            Some("hello")
        );
        assert_eq!(
            n.get_range_original(Range::Normalized(hello_n.unwrap())),
            Some("Hello")
        );
        assert_eq!(n.get_range(Range::Original(6..11)), Some("hello"));
        assert_eq!(n.get_range_original(Range::Original(6..11)), Some("Hello"));
    }

    #[test]
    fn original_range() {
        let mut n = NormalizedString::from("Hello_______ World!");
        n.filter(|c| c != '_').lowercase();
        let world_n = n.get_range(Range::Normalized(6..11)).unwrap();
        let world_o = n.get_range_original(Range::Normalized(6..11)).unwrap();
        assert_eq!(world_n, "world");
        assert_eq!(world_o, "World");
        let original_range = Range::Original(n.convert_offsets(Range::Normalized(6..11)).unwrap());
        assert_eq!(n.get_range(original_range.clone()).unwrap(), "world");
        assert_eq!(
            n.get_range_original(original_range.clone()).unwrap(),
            "World"
        );
        assert_eq!(original_range.into_full_range(n.len_original()), 13..18);
    }

    #[test]
    fn added_around_edges() {
        let mut n = NormalizedString::from("Hello");
        n.transform(
            vec![
                (' ', 1),
                ('H', 0),
                ('e', 0),
                ('l', 0),
                ('l', 0),
                ('o', 0),
                (' ', 1),
            ]
            .into_iter(),
            0,
        );

        assert_eq!(&n.normalized, " Hello ");
        assert_eq!(
            n.get_range_original(Range::Normalized(1..n.normalized.len() - 1)),
            Some("Hello")
        );
    }

    #[test]
    fn remove_at_beginning() {
        let mut n = NormalizedString::from("     Hello");
        n.filter(|c| !c.is_whitespace());
        assert_eq!(
            n.get_range_original(Range::Normalized(1.."Hello".len())),
            Some("ello")
        );
        assert_eq!(
            n.get_range_original(Range::Normalized(0..n.normalized.len())),
            Some("Hello")
        );
    }

    #[test]
    fn remove_at_end() {
        let mut n = NormalizedString::from("Hello    ");
        n.filter(|c| !c.is_whitespace());
        assert_eq!(n.get_range_original(Range::Normalized(0..4)), Some("Hell"));
        assert_eq!(
            n.get_range_original(Range::Normalized(0..n.normalized.len())),
            Some("Hello")
        );
    }

    #[test]
    fn removed_around_both_edges() {
        let mut n = NormalizedString::from("  Hello  ");
        n.filter(|c| !c.is_whitespace());
        assert_eq!(&n.normalized, "Hello");

        assert_eq!(
            n.get_range_original(Range::Normalized(0.."Hello".len())),
            Some("Hello")
        );
        assert_eq!(
            n.get_range_original(Range::Normalized(1.."Hell".len())),
            Some("ell")
        );
    }

    #[test]
    fn lstrip() {
        let mut n = NormalizedString::from("  This is an example  ");
        n.lstrip();
        assert_eq!(&n.normalized, "This is an example  ");
        assert_eq!(
            n.get_range_original(Range::Normalized(0..n.normalized.len())),
            Some("This is an example  ")
        );
    }

    #[test]
    fn rstrip() {
        let mut n = NormalizedString::from("  This is an example  ");
        n.rstrip();
        assert_eq!(&n.normalized, "  This is an example");
        assert_eq!(
            n.get_range_original(Range::Normalized(0..n.normalized.len())),
            Some("  This is an example")
        );
    }

    #[test]
    fn strip() {
        let mut n = NormalizedString::from("  This is an example  ");
        n.strip();
        assert_eq!(&n.normalized, "This is an example");
        assert_eq!(
            n.get_range_original(Range::Normalized(0..n.normalized.len())),
            Some("This is an example")
        );
    }

    #[test]
    fn prepend() {
        let mut n = NormalizedString::from("there");
        n.prepend("Hey ");
        assert_eq!(&n.normalized, "Hey there");
        assert_eq!(
            n.alignments,
            vec![
                (0, 0),
                (0, 0),
                (0, 0),
                (0, 0),
                (0, 1),
                (1, 2),
                (2, 3),
                (3, 4),
                (4, 5)
            ]
        );
        assert_eq!(n.convert_offsets(Range::Normalized(0..4)), Some(0..0));
    }

    #[test]
    fn append() {
        let mut n = NormalizedString::from("Hey");
        n.append(" there");
        assert_eq!(&n.normalized, "Hey there");
        assert_eq!(
            n.alignments,
            vec![
                (0, 1),
                (1, 2),
                (2, 3),
                (3, 3),
                (3, 3),
                (3, 3),
                (3, 3),
                (3, 3),
                (3, 3)
            ]
        );
        assert_eq!(
            n.convert_offsets(Range::Normalized(3.." there".len())),
            Some(3..3)
        );
    }

    #[test]
    fn get_range() {
        let s = String::from("Hello my name is John 👋");
        assert_eq!(get_range_of(&s, ..), Some(&s[..]));
        assert_eq!(get_range_of(&s, 17..), Some("John 👋"));
    }

    #[test]
    fn merge() {
        let mut s = NormalizedString::from("A sentence that will be merged");
        s.prepend(" ");

        let mut merged = NormalizedString::from("A sentence");
        let s2 = NormalizedString::from(" that will");
        let s3 = NormalizedString::from(" be merged");
        merged.prepend(" ");
        merged.merge_with(&s2);
        merged.merge_with(&s3);

        assert_eq!(s, merged);
    }

    #[test]
    fn slice() {
        let mut s = NormalizedString::from("𝔾𝕠𝕠𝕕 𝕞𝕠𝕣𝕟𝕚𝕟𝕘");
        s.nfkc();

        assert_eq!(
            s.slice(Range::Original(0..4)),
            Some(NormalizedString {
                original: "𝔾𝕠𝕠𝕕".to_string(),
                normalized: "Good".to_string(),
                alignments: vec![(0, 1), (1, 2), (2, 3), (3, 4)]
            })
        );
        assert_eq!(
            s.slice(Range::Normalized(0..4)),
            Some(NormalizedString {
                original: "𝔾𝕠𝕠𝕕".to_string(),
                normalized: "Good".to_string(),
                alignments: vec![(0, 1), (1, 2), (2, 3), (3, 4)]
            })
        );

        // Make sure the sliced NormalizedString is still aligned as expected
        let mut s = NormalizedString::from("   Good Morning!   ");
        s.strip();

        // If we keep the whole slice
        let slice = s.slice(Range::Original(..)).unwrap();
        assert_eq!(
            slice.get_range_original(Range::Normalized(0..4)),
            Some("Good")
        );
        let slice = s.slice(Range::Normalized(..)).unwrap();
        assert_eq!(
            slice.get_range_original(Range::Normalized(0..4)),
            Some("Good")
        );

        // If we keep after the modified piece
        let slice = s.slice(Range::Original(4..15)).unwrap();
        assert_eq!(
            slice.get_range_original(Range::Normalized(0..3)),
            Some("ood")
        );

        // If we keep only the modified piece
        let slice = s.slice(Range::Original(3..16)).unwrap();
        assert_eq!(
            slice.get_range_original(Range::Normalized(0..4)),
            Some("Good")
        );
    }

    #[test]
    fn slice_bytes() {
        let mut s = NormalizedString::from("𝔾𝕠𝕠𝕕 𝕞𝕠𝕣𝕟𝕚𝕟𝕘");
        s.nfkc();

        assert_eq!(
            s.slice_bytes(Range::Original(0..16)),
            Some(NormalizedString {
                original: "𝔾𝕠𝕠𝕕".to_string(),
                normalized: "Good".to_string(),
                alignments: vec![(0, 1), (1, 2), (2, 3), (3, 4)]
            })
        );
        assert_eq!(
            s.slice_bytes(Range::Original(17..)),
            Some(NormalizedString {
                original: "𝕞𝕠𝕣𝕟𝕚𝕟𝕘".to_string(),
                normalized: "morning".to_string(),
                alignments: vec![(0, 1), (1, 2), (2, 3), (3, 4), (4, 5), (5, 6), (6, 7)]
            })
        );
        assert_eq!(
            s.slice_bytes(Range::Normalized(0..4)),
            Some(NormalizedString {
                original: "𝔾𝕠𝕠𝕕".to_string(),
                normalized: "Good".to_string(),
                alignments: vec![(0, 1), (1, 2), (2, 3), (3, 4)]
            })
        );
        assert_eq!(s.slice_bytes(Range::Original(0..10)), None);
    }
}

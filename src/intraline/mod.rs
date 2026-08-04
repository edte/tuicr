//! Delta-compatible within-line edit inference.
//!
//! The token alignment and greedy homologous-line matching are adapted from
//! delta's `align.rs` and `edits.rs` (Copyright 2020 Dan Davison, MIT). The
//! complete license notice is retained in `align.rs`.

mod align;

use std::ops::Range;

use regex::Regex;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::model::{DiffLine, LineOrigin};
use align::{Alignment, Operation};

pub(crate) const DEFAULT_WORD_DIFF_REGEX: &str = r"\w+";
pub(crate) const DEFAULT_MAX_LINE_DISTANCE: f64 = 0.6;
pub(crate) const DEFAULT_MAX_NAIVE_LINE_DISTANCE: f64 = 0.0;
pub(crate) const DEFAULT_MAX_LINE_LENGTH: usize = 3000;

#[derive(Clone, Debug)]
pub(crate) struct Config {
    pub enabled: bool,
    pub tokenization_regex: Regex,
    pub max_line_distance: f64,
    pub max_line_distance_for_naively_paired_lines: f64,
    pub max_line_length: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            tokenization_regex: Regex::new(DEFAULT_WORD_DIFF_REGEX)
                .expect("default word diff regex must compile"),
            max_line_distance: DEFAULT_MAX_LINE_DISTANCE,
            max_line_distance_for_naively_paired_lines: DEFAULT_MAX_NAIVE_LINE_DISTANCE,
            max_line_length: DEFAULT_MAX_LINE_LENGTH,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Section {
    pub range: Range<usize>,
    pub emphasized: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Block {
    pub deletion_start: usize,
    pub addition_start: usize,
    pub deletion_sections: Vec<Vec<Section>>,
    pub addition_sections: Vec<Vec<Section>>,
    pub alignment: Vec<(Option<usize>, Option<usize>)>,
}

impl Block {
    pub fn absolute_alignment(&self) -> impl Iterator<Item = (Option<usize>, Option<usize>)> + '_ {
        self.alignment.iter().map(|(deletion, addition)| {
            (
                deletion.map(|index| self.deletion_start + index),
                addition.map(|index| self.addition_start + index),
            )
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct HunkDiff {
    blocks: Vec<Block>,
}

impl HunkDiff {
    pub(crate) fn infer(lines: &[DiffLine], config: &Config) -> Self {
        let mut blocks = Vec::new();
        let mut index = 0;

        while index < lines.len() {
            if lines[index].origin != LineOrigin::Deletion {
                index += 1;
                continue;
            }

            let deletion_start = index;
            while index < lines.len() && lines[index].origin == LineOrigin::Deletion {
                index += 1;
            }
            let addition_start = index;
            while index < lines.len() && lines[index].origin == LineOrigin::Addition {
                index += 1;
            }

            let deletions = &lines[deletion_start..addition_start];
            let additions = &lines[addition_start..index];
            let deletion_prefixes: Vec<&str> = deletions
                .iter()
                .map(|line| analysis_prefix(&line.content, config.max_line_length))
                .collect();
            let addition_prefixes: Vec<&str> = additions
                .iter()
                .map(|line| analysis_prefix(&line.content, config.max_line_length))
                .collect();

            let (mut deletion_sections, mut addition_sections, alignment) = infer_edits(
                &deletion_prefixes,
                &addition_prefixes,
                &config.tokenization_regex,
                config.max_line_distance,
                config.max_line_distance_for_naively_paired_lines,
            );

            for (sections, line) in deletion_sections.iter_mut().zip(deletions) {
                extend_to_original_line(sections, line.content.len());
            }
            for (sections, line) in addition_sections.iter_mut().zip(additions) {
                extend_to_original_line(sections, line.content.len());
            }

            blocks.push(Block {
                deletion_start,
                addition_start,
                deletion_sections,
                addition_sections,
                alignment,
            });
        }

        Self { blocks }
    }

    pub(crate) fn block_at(&self, deletion_start: usize) -> Option<&Block> {
        self.blocks
            .iter()
            .find(|block| block.deletion_start == deletion_start)
    }

    pub(crate) fn sections_for_line(&self, line_index: usize) -> Option<&[Section]> {
        for block in &self.blocks {
            let deletion_end = block.deletion_start + block.deletion_sections.len();
            if (block.deletion_start..deletion_end).contains(&line_index) {
                return Some(&block.deletion_sections[line_index - block.deletion_start]);
            }

            let addition_end = block.addition_start + block.addition_sections.len();
            if (block.addition_start..addition_end).contains(&line_index) {
                return Some(&block.addition_sections[line_index - block.addition_start]);
            }
        }
        None
    }
}

pub(crate) fn aligned_block(
    lines: &[DiffLine],
    deletion_start: usize,
    diff: Option<&HunkDiff>,
) -> (usize, LineAlignment) {
    let mut deletion_end = deletion_start;
    while deletion_end < lines.len() && lines[deletion_end].origin == LineOrigin::Deletion {
        deletion_end += 1;
    }
    let addition_start = deletion_end;
    let mut addition_end = addition_start;
    while addition_end < lines.len() && lines[addition_end].origin == LineOrigin::Addition {
        addition_end += 1;
    }

    let deletion_count = deletion_end - deletion_start;
    let addition_count = addition_end - addition_start;
    let alignment = diff
        .and_then(|diff| diff.block_at(deletion_start))
        .map(|block| block.absolute_alignment().collect())
        .unwrap_or_else(|| {
            (0..deletion_count.max(addition_count))
                .map(|offset| {
                    (
                        (offset < deletion_count).then_some(deletion_start + offset),
                        (offset < addition_count).then_some(addition_start + offset),
                    )
                })
                .collect()
        });

    (addition_end, alignment)
}

fn analysis_prefix(line: &str, max_line_length: usize) -> &str {
    if max_line_length == 0 || line.len() <= max_line_length {
        return line;
    }
    let mut end = max_line_length;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    &line[..end]
}

fn extend_to_original_line(sections: &mut Vec<Section>, original_len: usize) {
    let covered = sections.last().map_or(0, |section| section.range.end);
    push_section(sections, false, covered..original_len);
}

type AnnotatedLines = Vec<Vec<Section>>;
type LineAlignment = Vec<(Option<usize>, Option<usize>)>;

fn infer_edits(
    deletion_lines: &[&str],
    addition_lines: &[&str],
    tokenization_regex: &Regex,
    max_line_distance: f64,
    max_line_distance_for_naively_paired_lines: f64,
) -> (AnnotatedLines, AnnotatedLines, LineAlignment) {
    let mut annotated_deletions = Vec::with_capacity(deletion_lines.len());
    let mut annotated_additions = Vec::with_capacity(addition_lines.len());
    let mut line_alignment = Vec::new();
    let mut addition_index = 0;

    'deletion_lines: for (deletion_index, deletion_line) in deletion_lines.iter().enumerate() {
        let mut considered = 0;
        for addition_line in &addition_lines[addition_index..] {
            let alignment = Alignment::new(
                tokenize(deletion_line, tokenization_regex),
                tokenize(addition_line, tokenization_regex),
            );
            let (deletion_sections, addition_sections, distance) =
                annotate(alignment, deletion_line, addition_line);
            let naively_paired = deletion_lines.len() == addition_lines.len()
                && distance <= max_line_distance_for_naively_paired_lines;
            if naively_paired || distance <= max_line_distance {
                for addition_line in &addition_lines[addition_index..addition_index + considered] {
                    annotated_additions.push(unchanged_line(addition_line));
                    line_alignment.push((None, Some(addition_index)));
                    addition_index += 1;
                }
                annotated_deletions.push(deletion_sections);
                annotated_additions.push(addition_sections);
                line_alignment.push((Some(deletion_index), Some(addition_index)));
                addition_index += 1;
                continue 'deletion_lines;
            }
            considered += 1;
        }

        annotated_deletions.push(unchanged_line(deletion_line));
        line_alignment.push((Some(deletion_index), None));
    }

    for addition_line in &addition_lines[addition_index..] {
        annotated_additions.push(unchanged_line(addition_line));
        line_alignment.push((None, Some(addition_index)));
        addition_index += 1;
    }

    (annotated_deletions, annotated_additions, line_alignment)
}

fn unchanged_line(line: &str) -> Vec<Section> {
    if line.is_empty() {
        Vec::new()
    } else {
        vec![Section {
            range: 0..line.len(),
            emphasized: false,
        }]
    }
}

fn tokenize<'a>(line: &'a str, regex: &Regex) -> Vec<&'a str> {
    let mut tokens = vec![""];
    let mut offset = 0;
    for matched in regex.find_iter(line) {
        if offset == 0 && matched.start() > 0 {
            tokens.push("");
        }
        tokens.extend(line[offset..matched.start()].graphemes(true));
        tokens.push(&line[matched.start()..matched.end()]);
        offset = matched.end();
    }
    if offset < line.len() {
        if offset == 0 {
            tokens.push("");
        }
        tokens.extend(line[offset..].graphemes(true));
    }
    tokens
}

fn annotate(
    alignment: Alignment<'_>,
    deletion_line: &str,
    addition_line: &str,
) -> (Vec<Section>, Vec<Section>, f64) {
    let (mut deletion_token_offset, mut addition_token_offset) = (0, 0);
    let (mut deletion_line_offset, mut addition_line_offset) = (0, 0);
    let (mut distance_numerator, mut distance_denominator) = (0, 0);
    let (mut deletion_was_emphasized, mut addition_was_emphasized) = (false, false);
    let mut deletion_sections = Vec::new();
    let mut addition_sections = Vec::new();

    for (operation, count) in alignment.coalesced_operations() {
        match operation {
            Operation::Deletion => {
                let range = section_range(
                    count,
                    &mut deletion_line_offset,
                    &mut deletion_token_offset,
                    &alignment.x,
                );
                let width = section_width(&deletion_line[range.clone()]);
                distance_denominator += width;
                distance_numerator += width;
                push_section(&mut deletion_sections, true, range);
                deletion_was_emphasized = true;
            }
            Operation::NoOp => {
                let deletion_range = section_range(
                    count,
                    &mut deletion_line_offset,
                    &mut deletion_token_offset,
                    &alignment.x,
                );
                let addition_range = section_range(
                    count,
                    &mut addition_line_offset,
                    &mut addition_token_offset,
                    &alignment.y,
                );
                let deletion_text = &deletion_line[deletion_range.clone()];
                distance_denominator += 2 * section_width(deletion_text);
                let is_space = deletion_text.trim().is_empty();
                let not_at_end = deletion_token_offset < alignment.x.len() - 1
                    || addition_token_offset < alignment.y.len() - 1;
                let emphasize_space = is_space
                    && ((deletion_was_emphasized && addition_was_emphasized && not_at_end)
                        || (!deletion_was_emphasized && !addition_was_emphasized));
                push_section(&mut deletion_sections, emphasize_space, deletion_range);
                push_section(&mut addition_sections, emphasize_space, addition_range);
                deletion_was_emphasized = false;
                addition_was_emphasized = false;
            }
            Operation::Insertion => {
                let range = section_range(
                    count,
                    &mut addition_line_offset,
                    &mut addition_token_offset,
                    &alignment.y,
                );
                let width = section_width(&addition_line[range.clone()]);
                distance_denominator += width;
                distance_numerator += width;
                push_section(&mut addition_sections, true, range);
                addition_was_emphasized = true;
            }
        }
    }

    debug_assert_eq!(deletion_line_offset, deletion_line.len());
    debug_assert_eq!(addition_line_offset, addition_line.len());
    let distance = if distance_denominator == 0 {
        0.0
    } else {
        distance_numerator as f64 / distance_denominator as f64
    };
    (deletion_sections, addition_sections, distance)
}

fn section_range(
    count: usize,
    line_offset: &mut usize,
    token_offset: &mut usize,
    tokens: &[&str],
) -> Range<usize> {
    let length: usize = tokens[*token_offset..*token_offset + count]
        .iter()
        .map(|token| token.len())
        .sum();
    let start = *line_offset;
    *line_offset += length;
    *token_offset += count;
    start..*line_offset
}

fn section_width(section: &str) -> usize {
    UnicodeWidthStr::width(section.trim())
}

fn push_section(sections: &mut Vec<Section>, emphasized: bool, range: Range<usize>) {
    if range.is_empty() {
        return;
    }
    if let Some(last) = sections.last_mut()
        && last.emphasized == emphasized
        && last.range.end == range.start
    {
        last.range.end = range.end;
        return;
    }
    sections.push(Section { range, emphasized });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config {
            max_line_distance: 1.0,
            ..Config::default()
        }
    }

    fn line(origin: LineOrigin, content: &str) -> DiffLine {
        DiffLine {
            origin,
            content: content.to_string(),
            old_lineno: None,
            new_lineno: None,
            highlighted_spans: None,
        }
    }

    #[test]
    fn should_highlight_changed_word_like_delta() {
        let lines = vec![
            line(LineOrigin::Deletion, "d.iteritems()"),
            line(LineOrigin::Addition, "d.items()"),
        ];
        let diff = HunkDiff::infer(&lines, &config());
        assert_eq!(
            diff.sections_for_line(0),
            Some(
                [
                    Section {
                        range: 0..2,
                        emphasized: false,
                    },
                    Section {
                        range: 2..11,
                        emphasized: true,
                    },
                    Section {
                        range: 11..13,
                        emphasized: false,
                    },
                ]
                .as_slice()
            )
        );
        assert_eq!(
            diff.sections_for_line(1),
            Some(
                [
                    Section {
                        range: 0..2,
                        emphasized: false,
                    },
                    Section {
                        range: 2..7,
                        emphasized: true,
                    },
                    Section {
                        range: 7..9,
                        emphasized: false,
                    },
                ]
                .as_slice()
            )
        );
    }

    #[test]
    fn should_align_homologous_lines_greedily() {
        let lines = vec![
            line(LineOrigin::Deletion, "aaaa a aaa"),
            line(LineOrigin::Deletion, "bbbb b bbb"),
            line(LineOrigin::Deletion, "cccc c ccc"),
            line(LineOrigin::Addition, "bbbb ! bbb"),
            line(LineOrigin::Addition, "dddd d ddd"),
            line(LineOrigin::Addition, "cccc ! ccc"),
        ];
        let cfg = Config {
            max_line_distance: 0.66,
            ..Config::default()
        };
        let diff = HunkDiff::infer(&lines, &cfg);
        let block = diff.block_at(0).unwrap();
        assert_eq!(
            block.alignment,
            vec![
                (Some(0), None),
                (Some(1), Some(0)),
                (None, Some(1)),
                (Some(2), Some(2)),
            ]
        );
        assert_eq!(
            block.absolute_alignment().collect::<Vec<_>>(),
            vec![
                (Some(0), None),
                (Some(1), Some(3)),
                (None, Some(4)),
                (Some(2), Some(5)),
            ]
        );
    }

    #[test]
    fn should_fall_back_to_positional_alignment_when_word_diff_is_disabled() {
        let lines = vec![
            line(LineOrigin::Deletion, "old one"),
            line(LineOrigin::Deletion, "old two"),
            line(LineOrigin::Addition, "new one"),
        ];

        assert_eq!(
            aligned_block(&lines, 0, None),
            (3, vec![(Some(0), Some(2)), (Some(1), None)])
        );
    }

    #[test]
    fn should_keep_unicode_ranges_on_grapheme_boundaries() {
        let lines = vec![
            line(LineOrigin::Deletion, "状态👨‍👩‍👧：旧"),
            line(LineOrigin::Addition, "状态👨‍👩‍👧：新"),
        ];
        let diff = HunkDiff::infer(&lines, &config());
        for (index, source) in ["状态👨‍👩‍👧：旧", "状态👨‍👩‍👧：新"].iter().enumerate()
        {
            for section in diff.sections_for_line(index).unwrap() {
                assert!(source.is_char_boundary(section.range.start));
                assert!(source.is_char_boundary(section.range.end));
            }
        }
    }

    #[test]
    fn should_limit_analysis_without_dropping_long_line_tail() {
        let lines = vec![
            line(LineOrigin::Deletion, "prefix-old-tail"),
            line(LineOrigin::Addition, "prefix-new-tail"),
        ];
        let mut cfg = config();
        cfg.max_line_length = 10;
        let diff = HunkDiff::infer(&lines, &cfg);
        assert_eq!(
            diff.sections_for_line(0).unwrap().last().unwrap().range.end,
            "prefix-old-tail".len()
        );
        assert!(
            !diff
                .sections_for_line(0)
                .unwrap()
                .last()
                .unwrap()
                .emphasized
        );
    }
}

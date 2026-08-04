use ratatui::{style::Style, text::Span};

use crate::{
    intraline::Section,
    model::{DiffLine, LineOrigin},
    theme::Theme,
    ui::styles,
};

pub(crate) fn styled_content(
    theme: &Theme,
    line: &DiffLine,
    origin: LineOrigin,
    sections: Option<&[Section]>,
) -> Vec<(Style, String)> {
    let base_style = match origin {
        LineOrigin::Addition => styles::diff_add_style(theme),
        LineOrigin::Deletion => styles::diff_del_style(theme),
        LineOrigin::Context => styles::diff_context_style(theme),
    };
    let source = line
        .highlighted_spans
        .clone()
        .unwrap_or_else(|| vec![(base_style, line.content.clone())]);

    let Some(sections) = sections else {
        return source;
    };
    if origin == LineOrigin::Context || !sections.iter().any(|section| section.emphasized) {
        return source;
    }
    if source
        .iter()
        .map(|(_, text)| text.as_str())
        .collect::<String>()
        != line.content
    {
        return vec![(base_style, line.content.clone())];
    }

    let emphasis_bg = match origin {
        LineOrigin::Addition => theme.diff_add_emph_bg,
        LineOrigin::Deletion => theme.diff_del_emph_bg,
        LineOrigin::Context => unreachable!(),
    };
    let mut output = Vec::new();
    let mut source_start = 0;

    for (style, text) in source {
        let source_end = source_start + text.len();
        let mut local_start = 0;

        for section in sections {
            if section.range.end <= source_start || section.range.start >= source_end {
                continue;
            }
            let local_end = section.range.start.clamp(source_start, source_end) - source_start;
            push_segment(&mut output, style, &text, local_start, local_end);

            let overlap_start = section.range.start.max(source_start) - source_start;
            let overlap_end = section.range.end.min(source_end) - source_start;
            let section_style = if section.emphasized {
                style.bg(emphasis_bg)
            } else {
                style
            };
            push_segment(
                &mut output,
                section_style,
                &text,
                overlap_start,
                overlap_end,
            );
            local_start = overlap_end;
        }

        push_segment(&mut output, style, &text, local_start, text.len());
        source_start = source_end;
    }

    output
}

pub(crate) fn content_spans(
    theme: &Theme,
    line: &DiffLine,
    origin: LineOrigin,
    sections: Option<&[Section]>,
) -> Vec<Span<'static>> {
    styled_content(theme, line, origin, sections)
        .into_iter()
        .map(|(style, text)| Span::styled(text, style))
        .collect()
}

fn push_segment(
    output: &mut Vec<(Style, String)>,
    style: Style,
    text: &str,
    start: usize,
    end: usize,
) {
    if start < end {
        output.push((style, text[start..end].to_string()));
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Style};

    use super::*;

    fn line(content: &str, highlighted_spans: Option<Vec<(Style, String)>>) -> DiffLine {
        DiffLine {
            origin: LineOrigin::Addition,
            content: content.to_string(),
            old_lineno: None,
            new_lineno: Some(1),
            highlighted_spans,
        }
    }

    #[test]
    fn overlays_emphasis_without_losing_syntax_foreground() {
        let syntax = Style::default().fg(Color::Blue).bg(Color::Rgb(1, 2, 3));
        let result = styled_content(
            &Theme::dark(),
            &line("old_name", Some(vec![(syntax, "old_name".to_string())])),
            LineOrigin::Addition,
            Some(&[
                Section {
                    range: 0..4,
                    emphasized: false,
                },
                Section {
                    range: 4..8,
                    emphasized: true,
                },
            ]),
        );

        assert_eq!(result.len(), 2);
        assert_eq!(result[0], (syntax, "old_".to_string()));
        assert_eq!(result[1].0.fg, Some(Color::Blue));
        assert_eq!(result[1].0.bg, Some(Theme::dark().diff_add_emph_bg));
        assert_eq!(result[1].1, "name");
    }

    #[test]
    fn splits_unicode_only_at_valid_boundaries() {
        let result = styled_content(
            &Theme::dark(),
            &line("前后", None),
            LineOrigin::Addition,
            Some(&[
                Section {
                    range: 0.."前".len(),
                    emphasized: false,
                },
                Section {
                    range: "前".len().."前后".len(),
                    emphasized: true,
                },
            ]),
        );

        assert_eq!(
            result
                .iter()
                .map(|(_, text)| text.as_str())
                .collect::<String>(),
            "前后"
        );
        assert_eq!(result[1].1, "后");
    }

    #[test]
    fn keeps_original_spans_without_emphasis() {
        let syntax = vec![(Style::default().fg(Color::Yellow), "same".to_string())];
        assert_eq!(
            styled_content(
                &Theme::dark(),
                &line("same", Some(syntax.clone())),
                LineOrigin::Addition,
                None,
            ),
            syntax
        );
    }
}

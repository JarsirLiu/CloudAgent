use ratatui::text::Line;
use ratatui::text::Span;
use textwrap::Options;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Debug, Default)]
pub(super) struct WrapOptions<'a> {
    pub(super) width: usize,
    pub(super) initial_indent: Line<'a>,
    pub(super) subsequent_indent: Line<'a>,
}

impl<'a> WrapOptions<'a> {
    pub(super) fn new(width: usize) -> Self {
        Self {
            width,
            initial_indent: Line::default(),
            subsequent_indent: Line::default(),
        }
    }

    pub(super) fn initial_indent(mut self, indent: Line<'a>) -> Self {
        self.initial_indent = indent;
        self
    }

    pub(super) fn subsequent_indent(mut self, indent: Line<'a>) -> Self {
        self.subsequent_indent = indent;
        self
    }
}

pub(super) fn word_wrap_text<'a>(input: &str, options: WrapOptions<'a>) -> Vec<Line<'static>> {
    if input.trim().is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let base_width = options.width.max(8);

    for paragraph in input.split('\n') {
        if paragraph.trim().is_empty() {
            out.push(Line::default());
            continue;
        }

        let initial_indent = line_to_static(&options.initial_indent);
        let subsequent_indent = line_to_static(&options.subsequent_indent);
        let initial_text = line_text(&initial_indent);
        let subsequent_text = line_text(&subsequent_indent);

        let wrapped = textwrap::wrap(
            paragraph,
            Options::new(base_width)
                .initial_indent(&initial_text)
                .subsequent_indent(&subsequent_text),
        );

        for (index, segment) in wrapped.into_iter().enumerate() {
            let owned = segment.into_owned();
            let indent = if index == 0 {
                &initial_indent
            } else {
                &subsequent_indent
            };
            let indent_text = line_text(indent);
            let prefix_len = indent_text.len();
            let prefix = if owned.len() >= prefix_len {
                owned[..prefix_len].to_string()
            } else {
                owned.clone()
            };
            let content = owned
                .get(prefix.len()..)
                .map(ToOwned::to_owned)
                .unwrap_or_default();

            let mut spans = indent.spans.clone();
            if !content.is_empty() {
                spans.push(Span::raw(content));
            }
            out.push(Line::from(spans));
        }
    }

    while out.last().is_some_and(|line| {
        line.spans
            .iter()
            .all(|span| span.content.as_ref().trim().is_empty())
    }) {
        out.pop();
    }

    out
}

pub(super) fn word_wrap_spans<'a>(
    spans: &[Span<'a>],
    options: WrapOptions<'a>,
) -> Vec<Line<'static>> {
    if spans.is_empty() {
        return Vec::new();
    }

    let initial_indent = line_to_static(&options.initial_indent);
    let total_width = initial_indent.width()
        + spans
            .iter()
            .map(|span| span.content.as_ref().width())
            .sum::<usize>();
    if total_width <= options.width {
        let mut line_spans = initial_indent.spans;
        line_spans.extend(
            spans
                .iter()
                .map(|span| Span::styled(span.content.to_string(), span.style)),
        );
        return vec![Line::from(line_spans).style(initial_indent.style)];
    }

    let style = spans.first().map(|span| span.style).unwrap_or_default();
    let text = spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    word_wrap_text(&text, options)
        .into_iter()
        .map(|line| {
            let spans = line
                .spans
                .into_iter()
                .enumerate()
                .map(|(index, span)| {
                    if index == 0 {
                        span
                    } else {
                        Span::styled(span.content.into_owned(), style)
                    }
                })
                .collect::<Vec<_>>();
            Line::from(spans)
        })
        .collect()
}

fn line_to_static(line: &Line<'_>) -> Line<'static> {
    Line::from(
        line.spans
            .iter()
            .map(|span| Span::styled(span.content.to_string(), span.style))
            .collect::<Vec<_>>(),
    )
    .style(line.style)
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

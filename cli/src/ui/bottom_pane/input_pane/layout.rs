use ratatui::layout::Rect;

#[derive(Clone, Copy)]
pub(super) struct InputPaneLayout {
    pub(super) input_area: Rect,
    pub(super) composer_area: Rect,
    pub(super) completion_area: Option<Rect>,
}

pub(super) const STATUS_ROW_HEIGHT: u16 = 1;
pub(super) const COMPOSER_TOP_SPACER_HEIGHT: u16 = 1;
pub(super) const COMPOSER_BOTTOM_SPACER_HEIGHT: u16 = 0;
pub(super) const HINT_ROW_HEIGHT: u16 = 1;
pub(super) const INPUT_BLOCK_CHROME_HEIGHT: u16 = 2;

pub(super) fn compute_input_layout(
    area: Rect,
    composer_height: u16,
    completion_line_count: usize,
) -> InputPaneLayout {
    let input_content_height = STATUS_ROW_HEIGHT
        .saturating_add(COMPOSER_TOP_SPACER_HEIGHT)
        .saturating_add(composer_height)
        .saturating_add(if completion_line_count == 0 {
            COMPOSER_BOTTOM_SPACER_HEIGHT.saturating_add(HINT_ROW_HEIGHT)
        } else {
            0
        });
    let input_height = input_content_height.saturating_add(INPUT_BLOCK_CHROME_HEIGHT);
    let (input_area, completion_area) = if completion_line_count == 0 {
        (area, None)
    } else {
        let requested = (completion_line_count as u16).saturating_add(1);
        let input_height = input_height.min(area.height);
        let completion_height = requested.min(area.height.saturating_sub(input_height));
        let input_area = Rect {
            height: input_height,
            ..area
        };
        let completion_area = (completion_height > 0).then_some(Rect {
            x: area.x,
            y: area.y.saturating_add(input_height),
            width: area.width,
            height: completion_height,
        });
        (input_area, completion_area)
    };

    let composer_area = Rect {
        x: input_area.x.saturating_add(1),
        y: input_area
            .y
            .saturating_add(1 + STATUS_ROW_HEIGHT + COMPOSER_TOP_SPACER_HEIGHT),
        width: input_area.width.saturating_sub(2),
        height: composer_height.min(input_area.height.saturating_sub(
            INPUT_BLOCK_CHROME_HEIGHT + STATUS_ROW_HEIGHT + COMPOSER_TOP_SPACER_HEIGHT,
        )),
    };

    InputPaneLayout {
        input_area,
        composer_area,
        completion_area,
    }
}

pub(super) fn compute_desired_height(composer_height: u16, completion_line_count: usize) -> u16 {
    let input_content_height = STATUS_ROW_HEIGHT
        .saturating_add(COMPOSER_TOP_SPACER_HEIGHT)
        .saturating_add(composer_height)
        .saturating_add(if completion_line_count == 0 {
            COMPOSER_BOTTOM_SPACER_HEIGHT.saturating_add(HINT_ROW_HEIGHT)
        } else {
            0
        });
    let input_height = input_content_height.saturating_add(INPUT_BLOCK_CHROME_HEIGHT);
    if completion_line_count == 0 {
        input_height.max(
            STATUS_ROW_HEIGHT
                .saturating_add(COMPOSER_TOP_SPACER_HEIGHT)
                .saturating_add(1)
                .saturating_add(COMPOSER_BOTTOM_SPACER_HEIGHT)
                .saturating_add(HINT_ROW_HEIGHT)
                .saturating_add(INPUT_BLOCK_CHROME_HEIGHT),
        )
    } else {
        input_height.saturating_add(completion_line_count as u16 + 1)
    }
}

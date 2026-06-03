//! Pure layout geometry for popups and modals (terminal-cell rects).

use ratatui::layout::Rect;

pub const MAX_SELECTOR_WIDTH: u16 = 82;
pub const MIN_SELECTOR_HEIGHT: u16 = 10;
pub const MAX_SELECTOR_HEIGHT: u16 = 34;
pub const POPUP_SHADOW_OFFSET: u16 = 1;

/// Center a rectangle inside `area`, clamped to fit with a 1-cell margin.
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2));
    let height = height.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

/// Selector/modal rect: capped width/height then centered in the terminal.
pub fn selector_area(area: Rect, width: u16, height: u16) -> Rect {
    centered(
        area,
        width.min(MAX_SELECTOR_WIDTH),
        height.clamp(MIN_SELECTOR_HEIGHT, MAX_SELECTOR_HEIGHT),
    )
}

/// One-cell offset shadow rect when it fits inside `bounds`.
pub fn shadow_area(area: Rect, bounds: Rect) -> Option<Rect> {
    let x = area.x.checked_add(POPUP_SHADOW_OFFSET)?;
    let y = area.y.checked_add(POPUP_SHADOW_OFFSET)?;
    let width = area.width.min(bounds.right().saturating_sub(x));
    let height = area.height.min(bounds.bottom().saturating_sub(y));
    (width > 0 && height > 0).then_some(Rect {
        x,
        y,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_is_symmetric_in_large_terminal() {
        let area = Rect::new(0, 0, 120, 40);
        let popup = centered(area, 80, 20);
        assert_eq!(popup.width, 80);
        assert_eq!(popup.height, 20);
        assert_eq!(popup.x, 20);
        assert_eq!(popup.y, 10);
    }

    #[test]
    fn centered_clamps_to_terminal_with_margin() {
        let area = Rect::new(0, 0, 30, 12);
        let popup = centered(area, 80, 20);
        assert_eq!(popup.width, 28);
        assert_eq!(popup.height, 10);
    }

    #[test]
    fn selector_area_caps_width_and_height() {
        let area = Rect::new(0, 0, 200, 80);
        let popup = selector_area(area, 120, 50);
        assert_eq!(popup.width, MAX_SELECTOR_WIDTH);
        assert_eq!(popup.height, MAX_SELECTOR_HEIGHT);
    }

    #[test]
    fn shadow_area_offsets_when_room_exists() {
        let bounds = Rect::new(0, 0, 100, 40);
        let popup = centered(bounds, 40, 12);
        let shadow = shadow_area(popup, bounds).expect("shadow fits");
        assert_eq!(shadow.x, popup.x + POPUP_SHADOW_OFFSET);
        assert_eq!(shadow.y, popup.y + POPUP_SHADOW_OFFSET);
    }
}

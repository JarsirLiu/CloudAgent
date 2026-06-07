use crossterm::event::{KeyEvent, MouseEvent};

pub(crate) enum UiEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Resize,
    Tick,
    Draw,
}

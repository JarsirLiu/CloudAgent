use crossterm::event::KeyEvent;

pub(crate) enum UiEvent {
    Key(KeyEvent),
    Paste(String),
    Resize,
    Tick,
    Draw,
}

#[derive(Default)]
enum State {
    /// Done!
    Done,

    #[default]
    Transition,
}

pub struct EntryFsm {
    state: State,
}

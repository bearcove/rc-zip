#[macro_export]
macro_rules! transition {
    ($state: expr => ($pattern: pat) $body: expr) => {
        $state = if let $pattern = std::mem::replace(&mut $state, State::Transitioning) {
            $body
        } else {
            unreachable!()
        };
    };
}

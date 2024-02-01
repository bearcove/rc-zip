#[macro_export]
macro_rules! transition {
    ($state: expr => ($pattern: pat) $body: expr) => {
        $state = if let $pattern = std::mem::take(&mut $state) {
            $body
        } else {
            unreachable!()
        };
    };
}

#[macro_export]
macro_rules! transition_async {
    ($state: expr => ($pattern: pat) $body: expr) => {
        *$state.as_mut() = if let $pattern = std::mem::take($state.as_mut().get_mut()) {
            $body
        } else {
            unreachable!()
        };
    };
}

#[macro_export]
macro_rules! transition {
    ($state: expr => ($pattern: pat) $body: expr) => {
        $state = if let $pattern = std::mem::replace(&mut $state, Default::default()) {
            $body
        } else {
            unreachable!()
        };
    };
}

#[macro_export]
macro_rules! transition_async {
    ($state: expr => ($pattern: pat) $body: expr) => {
        *$state.as_mut() = if let $pattern = std::mem::replace($state.get_mut(), Default::default())
        {
            $body
        } else {
            unreachable!()
        };
    };
}

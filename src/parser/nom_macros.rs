#[macro_export]
macro_rules! fields {
    (from $input:ident { $($name:ident : $combinator:expr),+ $(,)* } do $body:expr) => {
        |$input| {
            let ($input, ($($name),+)) = nom::sequence::tuple(($($combinator),+))($input)?;
            $body
        }
    };

    ({ $($name:ident : $combinator:expr),+ $(,)* } chain $next:expr) => {
        fields!(from i { $($name: $combinator,)+ } do $next(i))
    };

    ({ $($name:ident : $combinator:expr),+ $(,)* } map $body:expr) => {
        fields!(from i { $($name: $combinator,)+ } do {
            Ok((i, $body))
        })
    };

    ($struct:ident { $($name:ident : $combinator:expr),+ $(,)* }) => {
        fields!({ $($name: $combinator,)+ } map
            $struct { $($name,)+ }
        )
    };
}

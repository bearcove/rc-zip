use nom::{error::ErrorKind, IResult};

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

/// Result of a parse operation
///
/// Used internally when parsing, for example, the end of central directory record.
pub type Result<'a, T> = IResult<&'a [u8], T, Error<'a>>;

/// Parsing error, see [Error].
pub type Error<'a> = (&'a [u8], ErrorKind);

#[macro_use]
pub mod parser;
pub mod model;

#[macro_export]
macro_rules! def_string_line {
    ($func_name:ident, $tag:expr, $enum_name:ident, $variant:ident) => {
        fn $func_name(input: &str) -> IResult<&str, $enum_name> {
            use nom::bytes::complete::{tag, take_while, take_while1};
            use nom::character::complete::multispace0;
            map(
                delimited(
                    tag($tag),
                    delimited(multispace0, take_while1(|c| c != '\n'), multispace0),
                    take_while(|c| c == '\n'),
                ),
                |s: &str| $enum_name::$variant(s.trim().to_string()),
            )(input)
        }
    };
}

#[macro_use]
pub mod parser;
pub mod model;

#[macro_export]
macro_rules! def_string_line {
    ($func_name:ident, $tag:expr, $enum_name:ident, $variant:ident) => {
        fn $func_name(input: &[u8]) -> IResult<&[u8], $enum_name> {
            use nom::bytes::streaming::take_while;
            use nom::bytes::streaming::take_while1;
            use nom::character::complete::multispace0;
            map(
                delimited(
                    tag($tag),
                    delimited(multispace0, take_while1(|c| c != b'\n'), multispace0),
                    take_while(|c| c == b'\n'),
                ),
                |s: &[u8]| $enum_name::$variant(str::from_utf8(s).unwrap().trim().to_string()),
            )(input)
        }
    };
}

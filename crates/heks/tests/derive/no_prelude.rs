#![no_implicit_prelude]

#[derive(::heks::Bundle)]
struct Foo {
    foo: (),
}

#[derive(::heks::Bundle)]
struct Bar<T> {
    foo: T,
}

#[derive(::heks::Bundle)]
struct Baz;

#[derive(::heks::Query)]
struct Quux<'a> {
    foo: &'a (),
}

fn main() {}

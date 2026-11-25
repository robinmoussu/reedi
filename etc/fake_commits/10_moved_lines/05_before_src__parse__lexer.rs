pub struct Lexer<'a> {
    input: &'a str,
    position: usize,
}

fn is_identifier_start(character: char) -> bool {
    character.is_alphabetic() || character == '_'
}

fn is_identifier_continuation(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

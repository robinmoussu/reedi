pub fn is_identifier_start(character: char) -> bool {
    character.is_alphabetic() || character == '_' || character == '$'
}

fn is_identifier_continuation(character: char) -> bool {
    character.is_alphanumeric() || character == '_' || character == '$'
}

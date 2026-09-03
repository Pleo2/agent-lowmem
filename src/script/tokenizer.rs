use crate::result::Reason;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenizedScript {
    segments: Vec<CommandSegment>,
}

impl TokenizedScript {
    pub fn segments(&self) -> &[CommandSegment] {
        &self.segments
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSegment {
    arguments: Vec<String>,
}

impl CommandSegment {
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    BetweenWords,
    Unquoted,
    SingleQuoted,
    DoubleQuoted,
    AfterQuoted,
}

pub fn tokenize_script(bytes: &[u8]) -> Result<TokenizedScript, Reason> {
    std::str::from_utf8(bytes).map_err(|_| unsupported())?;

    let mut state = State::BetweenWords;
    let mut word = Vec::new();
    let mut arguments = Vec::new();
    let mut segments = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];
        match state {
            State::BetweenWords => match byte {
                b' ' | b'\t' => index += 1,
                b'\'' => {
                    state = State::SingleQuoted;
                    index += 1;
                }
                b'"' => {
                    state = State::DoubleQuoted;
                    index += 1;
                }
                b'&' if bytes.get(index + 1) == Some(&b'&') => {
                    push_segment(&mut segments, &mut arguments)?;
                    index += 2;
                }
                byte if safe_unquoted(byte) => {
                    word.push(byte);
                    state = State::Unquoted;
                    index += 1;
                }
                _ => return Err(unsupported()),
            },
            State::Unquoted => match byte {
                b' ' | b'\t' => {
                    push_word(&mut arguments, &mut word)?;
                    state = State::BetweenWords;
                    index += 1;
                }
                b'&' if bytes.get(index + 1) == Some(&b'&') => {
                    push_word(&mut arguments, &mut word)?;
                    push_segment(&mut segments, &mut arguments)?;
                    state = State::BetweenWords;
                    index += 2;
                }
                byte if safe_unquoted(byte) => {
                    word.push(byte);
                    index += 1;
                }
                _ => return Err(unsupported()),
            },
            State::SingleQuoted => match byte {
                b'\'' => {
                    push_word(&mut arguments, &mut word)?;
                    state = State::AfterQuoted;
                    index += 1;
                }
                b'\n' | b'\r' => return Err(unsupported()),
                _ => {
                    word.push(byte);
                    index += 1;
                }
            },
            State::DoubleQuoted => match byte {
                b'"' => {
                    push_word(&mut arguments, &mut word)?;
                    state = State::AfterQuoted;
                    index += 1;
                }
                b'\\' => match bytes.get(index + 1) {
                    Some(b'"') => {
                        word.push(b'"');
                        index += 2;
                    }
                    Some(b'\\') => {
                        word.push(b'\\');
                        index += 2;
                    }
                    _ => return Err(unsupported()),
                },
                b'$' | b'`' | b'\n' | b'\r' => return Err(unsupported()),
                _ => {
                    word.push(byte);
                    index += 1;
                }
            },
            State::AfterQuoted => match byte {
                b' ' | b'\t' => {
                    state = State::BetweenWords;
                    index += 1;
                }
                b'&' if bytes.get(index + 1) == Some(&b'&') => {
                    push_segment(&mut segments, &mut arguments)?;
                    state = State::BetweenWords;
                    index += 2;
                }
                _ => return Err(unsupported()),
            },
        }
    }

    match state {
        State::Unquoted => push_word(&mut arguments, &mut word)?,
        State::SingleQuoted | State::DoubleQuoted => return Err(unsupported()),
        State::BetweenWords | State::AfterQuoted => {}
    }
    push_segment(&mut segments, &mut arguments)?;
    Ok(TokenizedScript { segments })
}

fn push_word(arguments: &mut Vec<String>, word: &mut Vec<u8>) -> Result<(), Reason> {
    let decoded = String::from_utf8(std::mem::take(word)).map_err(|_| unsupported())?;
    arguments.push(decoded);
    Ok(())
}

fn push_segment(
    segments: &mut Vec<CommandSegment>,
    arguments: &mut Vec<String>,
) -> Result<(), Reason> {
    if arguments.is_empty() {
        return Err(unsupported());
    }
    segments.push(CommandSegment {
        arguments: std::mem::take(arguments),
    });
    Ok(())
}

const fn safe_unquoted(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'_' | b'@' | b'%' | b'+' | b'=' | b':' | b',' | b'.' | b'/' | b'-'
        )
}

const fn unsupported() -> Reason {
    Reason::ScriptSyntaxUnsupported
}

#[cfg(test)]
mod tests {
    use super::tokenize_script;
    use crate::result::Reason;

    #[test]
    fn tokenizes_safe_segments_without_reconstructing_shell_text() {
        let parsed = tokenize_script(br#"rimraf dist && vitest run 'src/*.test.ts'"#).unwrap();

        assert_eq!(parsed.segments().len(), 2);
        assert_eq!(parsed.segments()[0].arguments(), ["rimraf", "dist"]);
        assert_eq!(
            parsed.segments()[1].arguments(),
            ["vitest", "run", "src/*.test.ts"]
        );
    }

    #[test]
    fn accepts_the_complete_literal_grammar() {
        let parsed = tokenize_script(
            br#"tool azAZ09_@%+=:,./- -- '' 'literal $ ` \' "quote: \" and slash \\ and *?[{}#;|&<>~" && next ok"#,
        )
        .unwrap();

        assert_eq!(parsed.segments().len(), 2);
        assert_eq!(
            parsed.segments()[0].arguments(),
            [
                "tool",
                "azAZ09_@%+=:,./-",
                "--",
                "",
                "literal $ ` \\",
                "quote: \" and slash \\ and *?[{}#;|&<>~",
            ]
        );
        assert_eq!(parsed.segments()[1].arguments(), ["next", "ok"]);
    }

    #[test]
    fn accepts_separators_without_surrounding_spaces_and_ascii_tabs() {
        let parsed = tokenize_script(b"first\targ&&second -- value").unwrap();
        assert_eq!(parsed.segments()[0].arguments(), ["first", "arg"]);
        assert_eq!(parsed.segments()[1].arguments(), ["second", "--", "value"]);
    }

    #[test]
    fn rejects_every_unsupported_operator_and_expansion() {
        for script in [
            b"a | b".as_slice(),
            b"a || b",
            b"a &",
            b"a; b",
            b"a > out",
            b"a >> out",
            b"a < in",
            b"a 2>&1",
            b"a $VAR",
            b"a $(b)",
            b"a ${B}",
            b"a `b`",
            b"~tool run",
            b"a *",
            b"a ?",
            b"a [x]",
            b"a (b)",
            b"a {b}",
            b"a # comment",
            b"a \\ b",
        ] {
            assert_eq!(
                tokenize_script(script).unwrap_err(),
                Reason::ScriptSyntaxUnsupported,
                "script should be rejected: {script:?}"
            );
        }
    }

    #[test]
    fn rejects_invalid_boundaries_quotes_and_input_encoding() {
        for script in [
            b"".as_slice(),
            b"   ",
            b"&& a",
            b"a &&",
            b"a && && b",
            b"a\n b",
            b"a\r b",
            b"'unterminated",
            b"\"unterminated",
            b"\"bad \\q\"",
            b"\"bad $value\"",
            b"\"bad `value`\"",
            b"word'fragment'",
            b"'fragment'word",
            b"'one''two'",
            b"\xff",
        ] {
            assert_eq!(
                tokenize_script(script).unwrap_err(),
                Reason::ScriptSyntaxUnsupported,
                "script should be rejected: {script:?}"
            );
        }
    }

    #[test]
    fn errors_never_retain_or_print_invalid_script_contents() {
        let secret = "agent-lowmem-private-script-sentinel";
        let invalid = format!("tool ${secret}");
        let error = tokenize_script(invalid.as_bytes()).unwrap_err();
        let debug = format!("{error:?}");

        assert_eq!(error, Reason::ScriptSyntaxUnsupported);
        assert!(!debug.contains(secret));
        assert_eq!(debug, "ScriptSyntaxUnsupported");
    }
}

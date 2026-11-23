// Text cleanup between the model and the paste.
//
// Rules only, no LLM, all local, all fast. In order:
//   1. filler words ("um", "uh", ...) go away, punctuation is re-knit
//   2. spoken layout commands: "new line", "new paragraph"
//   3. whitespace and capitalization tidy-up

pub fn clean(raw: &str, fillers: bool) -> String {
    let mut s = raw.trim().to_string();
    if fillers {
        s = strip_fillers(&s);
    }
    s = layout_commands(&s);
    s = collapse_repeats(&s);
    tidy(&s)
}

/// The model sometimes stutters on a repeated word at the end of a short
/// chunk ("really really really really ..."). People do say a word three
/// times; nobody says it eight. Runs longer than three collapse to three.
fn collapse_repeats(s: &str) -> String {
    let core = |w: &str| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    let mut out: Vec<&str> = Vec::new();
    let mut run = 0;
    for w in s.split(' ') {
        let c = core(w);
        if !c.is_empty() && out.last().map(|p| core(p) == c).unwrap_or(false) {
            run += 1;
            if run >= 3 {
                continue;
            }
        } else {
            run = 0;
        }
        out.push(w);
    }
    out.join(" ")
}

const FILLERS: &[&str] = &["um", "umm", "uh", "uhh", "er", "erm", "ah", "hmm", "mm"];

/// Removes filler words together with any comma that was hugging them.
fn strip_fillers(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut tokens = s.split(' ').peekable();
    while let Some(tok) = tokens.next() {
        let core = tok.trim_matches(|c: char| !c.is_alphanumeric());
        let is_filler = !core.is_empty() && FILLERS.contains(&core.to_lowercase().as_str());
        if !is_filler {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(tok);
            continue;
        }
        // Keep sentence-ending punctuation that was attached to the filler
        // ("... um." -> "...") but drop commas.
        let tail: String = tok.chars().rev().take_while(|c| !c.is_alphanumeric()).collect::<Vec<_>>().into_iter().rev().collect();
        if tail.contains('.') || tail.contains('?') || tail.contains('!') {
            if out.ends_with(',') {
                out.pop();
            }
            if !out.is_empty() && !out.ends_with(['.', '?', '!']) {
                out.push(tail.chars().find(|c| ['.', '?', '!'].contains(c)).unwrap());
            }
        }
        // "Also, um, remind" -> "Also, remind": the comma after the filler is
        // dropped, the one before it stays.
        if let Some(next) = tokens.peek() {
            if next.is_empty() {
                tokens.next();
            }
        }
    }
    out
}

/// "new line" -> "\n", "new paragraph" -> "\n\n". Case-insensitive, and the
/// punctuation the model wrapped around the command is dropped.
fn layout_commands(s: &str) -> String {
    let mut out = s.to_string();
    for (cmd, rep) in [("new paragraph", "\n\n"), ("new line", "\n")] {
        let lower = out.to_lowercase();
        let mut result = String::with_capacity(out.len());
        let mut i = 0;
        while let Some(pos) = lower[i..].find(cmd) {
            let start = i + pos;
            let end = start + cmd.len();
            let before_ok = start == 0 || !lower.as_bytes()[start - 1].is_ascii_alphanumeric();
            let after_ok = end >= lower.len() || !lower.as_bytes()[end].is_ascii_alphanumeric();
            if before_ok && after_ok {
                // Strip trailing punctuation/space before the command and
                // leading punctuation/space after it.
                // Keep a sentence-ending mark before the command; drop commas.
                let mut head = out[i..start].to_string();
                while head.ends_with([' ', ',', ';', ':']) {
                    head.pop();
                }
                result.push_str(&head);
                result.push_str(rep);
                let mut j = end;
                while j < out.len() && out[j..].starts_with([' ', ',', '.', ';', ':']) {
                    j += 1;
                }
                i = j;
            } else {
                result.push_str(&out[i..end]);
                i = end;
            }
        }
        result.push_str(&out[i..]);
        out = result;
    }
    out
}

/// Collapse spaces, fix " ,", capitalize sentence starts and line starts.
fn tidy(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.chars() {
        if c == ' ' {
            if prev_space || out.is_empty() || out.ends_with('\n') {
                continue;
            }
            prev_space = true;
        } else {
            if matches!(c, ',' | '.' | '?' | '!' | ';' | ':') && out.ends_with(' ') {
                out.pop();
            }
            prev_space = false;
        }
        out.push(c);
    }
    // Capitalize after sentence end and at line starts.
    let mut result = String::with_capacity(out.len());
    let mut cap = true;
    for c in out.chars() {
        if cap && c.is_alphabetic() {
            result.extend(c.to_uppercase());
            cap = false;
        } else {
            result.push(c);
            if matches!(c, '.' | '?' | '!' | '\n') {
                cap = true;
            } else if !c.is_whitespace() {
                cap = false;
            }
        }
    }
    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fillers_go_away_and_commas_reknit() {
        assert_eq!(clean("Also, um, remind me.", true), "Also, remind me.");
        assert_eq!(clean("Um, so I think uh we should go.", true), "So I think we should go.");
        assert_eq!(clean("It was fine, um.", true), "It was fine.");
    }

    #[test]
    fn layout_commands() {
        assert_eq!(clean("First point. New line. Second point.", true), "First point.\nSecond point.");
        assert_eq!(clean("Hi Sam, new paragraph, thanks for today.", true), "Hi Sam\n\nThanks for today.");
        assert_eq!(clean("The newline character.", true), "The newline character.");
    }

    #[test]
    fn repeats_collapse() {
        assert_eq!(clean("really, really, really go deep", true), "Really, really, really go deep");
        assert_eq!(clean("really really really really really really go", true), "Really really really go");
    }

    #[test]
    fn tidy_spacing() {
        assert_eq!(clean("hello  ,  world .", true), "Hello, world.");
    }
}

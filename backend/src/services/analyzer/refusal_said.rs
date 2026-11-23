//! Why an endpoint refused the shape of a request, said on a bench's log:
//! the start of its answer, and how much the run had cost by then (what
//! follows is asked another way). Never what was asked, nor a key: whatever
//! of the answer is in them is left out.

/// As much of the answer as is said.
const SAID: usize = 300;
/// A run of the answer this long found in the request is the request's.
const QUOTED: usize = 12;

/// `answer`'s start without whatever of it is in `private` (what was asked,
/// the key), or names an account.
pub(super) fn said(answer: &str, private: &[&str]) -> String {
    // What the endpoint sent, past where it was asked and with which model.
    let body = answer.find('{').map_or(answer, |at| &answer[at..]);
    let start: Vec<char> = body.chars().take(SAID).collect();
    // An endpoint says why in English; what is not is someone's words.
    let mut hidden: Vec<bool> = start.iter().map(|char| !char.is_ascii()).collect();
    for at in 0..(start.len() + 1).saturating_sub(QUOTED) {
        let run: String = start[at..at + QUOTED].iter().collect();
        if private.iter().any(|text| text.contains(&run)) {
            hidden[at..at + QUOTED].fill(true);
        }
    }
    let mut out = String::new();
    for (char, hidden) in start.iter().zip(hidden) {
        match (hidden, out.ends_with('…')) {
            (true, false) => out.push('…'),
            (true, true) => {}
            (false, _) => out.push(*char),
        }
    }
    match regex::Regex::new(r#"(user_id|sk-|Bearer)[^,}\s]*[,}\s]?.*"#) {
        Ok(named) => named.replace(&out.replace('\n', " "), "…").to_string(),
        Err(_) => "…".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_is_said_without_what_was_asked_or_a_key() {
        let why = "400: output_config.format: Extra inputs are not permitted";
        assert_eq!(said(why, &["我下周二体检", "sk-or-v1-abcdef"]), why);
        let quoted = said(
            "400: cannot read 'my cat is named Luna and she' there",
            &["Today my cat is named Luna and she sleeps"],
        );
        assert!(quoted.starts_with("400: cannot read") && !quoted.contains("Luna"));
        assert!(!said("400 bad input: 我下周二体检", &[]).contains("体检"));
        let keyed = said(
            "400 bad, Bearer sk-or-v1-abcdef0123456789",
            &["sk-or-v1-abcdef0123456789"],
        );
        assert!(keyed.contains("400 bad") && !keyed.contains("sk-"));
        assert!(
            !said(r#"{"error":{"code":400},"user_id":"user_2abc"}"#, &[]).contains("user_2abc")
        );
        assert_eq!(said(&"x".repeat(900), &[]).chars().count(), SAID);
        let wrapped = format!("{} {{\"error\":1}}", "asked at an endpoint ".repeat(20));
        assert_eq!(said(&wrapped, &[]), "{\"error\":1}");
    }
}

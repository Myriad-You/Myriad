//! Bounded observations of already-emitted prose, never a gate on that prose.
const MAX_PREVIEW_CHARS: usize = 900;
const MAX_UNBROKEN_CHARS: usize = 96;

#[derive(Default)]
pub struct MotionPreview {
    text: String,
    chars: usize,
    pending: usize,
}

impl MotionPreview {
    pub fn push(&mut self, spoken: &str) -> Option<String> {
        let mut latest = None;
        for ch in spoken.chars() {
            if self.text.is_empty() && ch.is_whitespace() {
                continue;
            }
            self.text.push(ch);
            self.chars += 1;
            self.pending += 1;
            if self.chars > MAX_PREVIEW_CHARS {
                self.text
                    .drain(..self.text.chars().next().unwrap().len_utf8());
                self.chars -= 1;
            }
            let boundary = matches!(ch, '。' | '！' | '？' | '!' | '?' | '\n')
                || (ch.is_whitespace() && self.text.trim_end().ends_with('.'));
            if (self.pending >= 3 && boundary) || self.pending >= MAX_UNBROKEN_CHARS {
                latest = self.finish();
            }
        }
        latest
    }

    pub fn finish(&mut self) -> Option<String> {
        if self.pending == 0 {
            return None;
        }
        self.pending = 0;
        let text = self.text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_windows_continue_coalesce_and_retain_the_latest_text() {
        let mut preview = MotionPreview::default();
        assert!(preview.push("你好").is_none());
        assert_eq!(
            preview.push("，今天怎么样？后面仍在说。").as_deref(),
            Some("你好，今天怎么样？后面仍在说。")
        );
        let next = preview
            .push(&format!("{}最后的问题？", "字".repeat(10_000)))
            .unwrap();
        assert_eq!(next.chars().count(), MAX_PREVIEW_CHARS);
        assert!(next.ends_with("最后的问题？"));
        assert!(preview.finish().is_none());
    }

    #[test]
    fn short_unpunctuated_tail_is_observed_on_completion_once() {
        let mut preview = MotionPreview::default();
        assert!(preview.push("好").is_none());
        assert_eq!(preview.finish().as_deref(), Some("好"));
        assert!(preview.finish().is_none());
    }

    #[test]
    fn whitespace_decimals_and_urls_do_not_start_a_partial_floor() {
        let mut preview = MotionPreview::default();
        assert!(preview.push(&" \n".repeat(1000)).is_none());
        assert!(preview.push("The value is 3.").is_none());
        assert!(preview.push("14 and https://example.com").is_none());
        assert!(preview.push(" works!").unwrap().ends_with("works!"));
    }
}

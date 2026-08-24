pub fn is_safe_merope_output(content: &str) -> bool {
    let normalized = content.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return true;
    }

    ![
        "tell me your password",
        "send me your password",
        "share your password",
        "enter your password",
        "give me your token",
        "send me your token",
        "provide your api key",
        "share your api key",
        "i accessed your account",
        "i changed your settings",
        "i deleted your data",
        "告诉我你的密码",
        "把密码告诉我",
        "发给我你的密码",
        "输入你的密码",
        "提供你的密钥",
        "发给我你的令牌",
        "我访问了你的账户",
        "我修改了你的设置",
        "我删除了你的数据",
        "パスワードを教えて",
        "パスワードを送って",
        "パスワードを入力して",
        "apiキーを教えて",
        "トークンを送って",
        "あなたのアカウントにアクセスしました",
        "設定を変更しました",
        "データを削除しました",
    ]
    .iter()
    .any(|forbidden| normalized.contains(forbidden))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_credential_requests_and_false_site_action_claims() {
        assert!(!is_safe_merope_output("Tell me your password."));
        assert!(!is_safe_merope_output("把密码告诉我"));
        assert!(!is_safe_merope_output("設定を変更しました"));
        assert!(!is_safe_merope_output("I deleted your data."));
    }

    #[test]
    fn allows_normal_security_advice_and_conversation() {
        assert!(is_safe_merope_output(
            "Never share passwords or API keys with anyone."
        ));
        assert!(is_safe_merope_output("今天也要记得休息。"));
    }
}

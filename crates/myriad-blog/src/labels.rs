//! 平台写进博客页面的字：友链页、文章列表和分类/标签/作者页、评论框和伴读的标题、404 页。
//! 跟着博客的语言走，和站点界面同一套说法（frontend/src/i18n/embed.*.json）。

pub(crate) struct Labels {
    pub friends: &'static str,
    pub posts: &'static str,
    pub categories: &'static str,
    pub tags: &'static str,
    pub authors: &'static str,
    pub comments: &'static str,
    pub friend_apply: &'static str,
    pub companion: &'static str,
    pub not_found: &'static str,
    pub not_found_body: &'static str,
    pub home: &'static str,
}

const ZH_HANS: Labels = Labels {
    friends: "友链",
    posts: "文章",
    categories: "分类",
    tags: "标签",
    authors: "作者",
    comments: "评论",
    friend_apply: "友联申请",
    companion: "AI 伴读",
    not_found: "找不到页面",
    not_found_body: "这个地址没有内容。",
    home: "回到首页",
};

const ZH_HANT: Labels = Labels {
    friends: "友情連結",
    posts: "文章",
    categories: "分類",
    tags: "標籤",
    authors: "作者",
    comments: "評論",
    friend_apply: "友聯申請",
    companion: "AI 伴讀",
    not_found: "找不到頁面",
    not_found_body: "這個地址沒有內容。",
    home: "回到首頁",
};

const JA: Labels = Labels {
    friends: "リンク",
    posts: "記事",
    categories: "カテゴリー",
    tags: "タグ",
    authors: "著者",
    comments: "コメント",
    friend_apply: "相互リンク申請",
    companion: "AI 伴読",
    not_found: "ページが見つかりません",
    not_found_body: "このアドレスには何もありません。",
    home: "トップへ戻る",
};

const KO: Labels = Labels {
    friends: "링크",
    posts: "글",
    categories: "분류",
    tags: "태그",
    authors: "작성자",
    comments: "댓글",
    friend_apply: "링크 신청",
    companion: "AI 함께 읽기",
    not_found: "페이지를 찾을 수 없습니다",
    not_found_body: "이 주소에는 내용이 없습니다.",
    home: "처음으로",
};

const FR: Labels = Labels {
    friends: "Liens",
    posts: "Articles",
    categories: "Catégories",
    tags: "Étiquettes",
    authors: "Auteurs",
    comments: "Commentaires",
    friend_apply: "Demande de lien",
    companion: "Lecture IA",
    not_found: "Page introuvable",
    not_found_body: "Il n’y a rien à cette adresse.",
    home: "Retour à l’accueil",
};

const DE: Labels = Labels {
    friends: "Links",
    posts: "Beiträge",
    categories: "Kategorien",
    tags: "Schlagwörter",
    authors: "Autoren",
    comments: "Kommentare",
    friend_apply: "Linkantrag",
    companion: "KI-Begleiter",
    not_found: "Seite nicht gefunden",
    not_found_body: "Unter dieser Adresse gibt es nichts.",
    home: "Zur Startseite",
};

const EN: Labels = Labels {
    friends: "Links",
    posts: "Posts",
    categories: "Categories",
    tags: "Tags",
    authors: "Authors",
    comments: "Comments",
    friend_apply: "Friend link application",
    companion: "AI companion",
    not_found: "Page not found",
    not_found_body: "There is nothing at this address.",
    home: "Back to home",
};

/// `zh-cn`、`zh-Hant`、`ja`、`en-US` 这类写法都认。认不出的语言用英文，空的用简体中文。
pub(crate) fn labels(locale: &str) -> &'static Labels {
    let locale = locale.trim().to_ascii_lowercase().replace('_', "-");
    let lang = locale.split('-').next().unwrap_or("");
    match lang {
        "" => &ZH_HANS,
        "zh" if ["tw", "hk", "mo", "hant"]
            .iter()
            .any(|tag| locale.split('-').skip(1).any(|part| part == *tag)) =>
        {
            &ZH_HANT
        }
        "zh" => &ZH_HANS,
        "ja" => &JA,
        "ko" => &KO,
        "fr" => &FR,
        "de" => &DE,
        _ => &EN,
    }
}

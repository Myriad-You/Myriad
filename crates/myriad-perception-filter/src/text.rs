//! Text rules for the owner's own money and health: the two kinds that are
//! easiest to recognise, kept deliberately narrow. Job hunting, romance and
//! the rest are left to the source table and the on-device model.
//!
//! They also hit public news, explainers and fiction that talk about money
//! or lab values; one page fewer carried is the accepted cost, and there is
//! no "looks like news" exception. Chinese numerals (`两万块`), a bare `万`
//! with no currency and holdings with no currency are not amounts.
//!
//! The text is NFKC-folded first, so full-width digits, currency signs and
//! colons read as their plain forms.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextKind {
    Finance,
    Health,
}

/// Which rule hit; never the text it hit on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TextHit {
    pub kind: TextKind,
    pub rule: &'static str,
}

/// An amount next to a word for the owner's own account.
pub const FINANCE_AMOUNT: &str = "finance.amount";
/// A holdings table: two holdings words and a signed percentage.
pub const FINANCE_HOLDINGS: &str = "finance.holdings";
/// Pay and offers: salary words next to an amount.
pub const FINANCE_PAY: &str = "finance.pay";
/// A lab item followed by its value and unit.
pub const HEALTH_LAB: &str = "health.lab";
/// A dose near a prescription word.
pub const HEALTH_PRESCRIPTION: &str = "health.prescription";
/// A form's diagnosis field.
pub const HEALTH_FORM: &str = "health.form";

/// How far apart, in characters, two parts of one rule may be when they
/// are not on the same line.
const NEAR: usize = 40;
/// How far after a lab item its value may start.
const LAB_NEAR: usize = 20;

fn re(source: &str) -> Regex {
    Regex::new(source).expect("text rule pattern")
}

const NUMBER: &str = r"[0-9](?:[0-9,.]|[ \u{00A0}][0-9])*";
const SCALE: &str = r"(?:\s?(?:万亿|千|万|亿|億|兆|(?i:billion|trillion)))?";

static AMOUNT: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r"(?:(?:S\$|US\$|HK\$|\$|¥|€|£|\bRMB|\bCNY|\bJPY|\bSGD|\bUSD)\s?{NUMBER}{SCALE}|{NUMBER}{SCALE}\s?(?:円|日元|美元|新币|新元|元|块|ドル|(?i:dollars?\b)))"
    ))
});

static OWN_ACCOUNT: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        "可用余额|余额|总资产|持仓|市值|浮动盈亏|盈亏|累计收益|实发|应发|工资|薪资|到手|应纳税|应补税|退税|欠款|待还|应还|还款|本期账单|最低还款|借款",
        "|残高|預金|総資産|評価額|保有|損益|差引支給額|支給額|手取り|給与|源泉徴収|納付|住民税|借入|返済|ご利用額|ご請求額",
        r"|(?i:\b(?:balance|total assets|holdings|market value|unrealized|P&L|gain/loss|net pay|gross pay|salary|take-home|tax due|tax refund|amount owed|outstanding|statement balance|minimum payment|loan|repayment)\b)",
    ))
});

static PAY: LazyLock<Regex> =
    LazyLock::new(|| re(r"月薪|年薪|(?i:\b(?:base salary|base pay|offer)\b)"));
/// `年収` counts only in an offer or a job change.
static ANNUAL: LazyLock<Regex> = LazyLock::new(|| re("年収"));
static ANNUAL_CONTEXT: LazyLock<Regex> = LazyLock::new(|| re("現職|内定"));

static HOLDINGS: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"份额|净值|成本|持仓|浮动盈亏|收益率|保有|口数|取得単価|評価損益|(?i:\b(?:holdings|qty|avg cost|unrealised|unrealized)\b)",
    )
});
static SIGNED_PERCENT: LazyLock<Regex> = LazyLock::new(|| re(r"[+\-−][0-9]+(?:\.[0-9]+)?\s?%"));

static LAB_ITEM: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"HbA1c|血糖|尿酸|胆固醇|コレステロール|\bLDL\b|\bHDL\b|\bALT\b|\bAST\b|γ-GTP|\bTSH\b|血压|血圧|(?i:\bglucose\b|\bcholesterol\b)|\beGFR\b|\bPSA\b|\bhCG\b",
    )
});
static LAB_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"[0-9]+(?:\.[0-9]+)?\s?(?i:mmol/L|mg/dL|mIU/mL|mIU/L|μIU/mL|IU/L|U/L|ng/mL|ng/dL|pg/mL|g/dL|mmHg|%)",
    )
});
static DOSE: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"[0-9]+(?:\.[0-9]+)?\s?(?:(?:mg|μg|mcg|ml|mL)\b|錠|片|粒|カプセル|(?i:\b(?:tablets?|capsules?)\b))",
    )
});
static PRESCRIPTION: LazyLock<Regex> =
    LazyLock::new(|| re(r"处方|処方|(?i:\bprescription\b)|\bRx\b|用法|服用|服药|薬局"));
static FORM: LazyLock<Regex> =
    LazyLock::new(|| re(r"诊断\s?:|临床诊断|診断名|診断\s?:|傷病名|(?i:\bdiagnosis\s?:)|\bDx\s?:"));

/// Every rule that hits, once each, in the order above.
pub fn text_hits(text: &str) -> Vec<TextHit> {
    let text: String = text.nfkc().collect();
    let mut hits = Vec::new();
    let mut hit = |kind, rule| {
        let found = TextHit { kind, rule };
        if !hits.contains(&found) {
            hits.push(found);
        }
    };
    if near(&text, &AMOUNT, &OWN_ACCOUNT) {
        hit(TextKind::Finance, FINANCE_AMOUNT);
    }
    if holdings(&text) {
        hit(TextKind::Finance, FINANCE_HOLDINGS);
    }
    let annual = ANNUAL_CONTEXT.is_match(&text) && near(&text, &AMOUNT, &ANNUAL);
    if annual || near(&text, &AMOUNT, &PAY) {
        hit(TextKind::Finance, FINANCE_PAY);
    }
    if lab(&text) {
        hit(TextKind::Health, HEALTH_LAB);
    }
    if near(&text, &DOSE, &PRESCRIPTION) {
        hit(TextKind::Health, HEALTH_PRESCRIPTION);
    }
    if FORM.is_match(&text) {
        hit(TextKind::Health, HEALTH_FORM);
    }
    hits
}

/// A match of `a` and one of `b` on the same line, or at most [`NEAR`]
/// characters apart.
fn near(text: &str, a: &Regex, b: &Regex) -> bool {
    let b: Vec<(usize, usize)> = b.find_iter(text).map(|m| (m.start(), m.end())).collect();
    a.find_iter(text).any(|a| {
        b.iter().any(|&(start, end)| {
            let (gap_from, gap_to) = if end <= a.start() {
                (end, a.start())
            } else if a.end() <= start {
                (a.end(), start)
            } else {
                return true;
            };
            let gap = &text[gap_from..gap_to];
            !gap.contains('\n') || gap.chars().count() <= NEAR
        })
    })
}

fn holdings(text: &str) -> bool {
    let mut words: Vec<String> = HOLDINGS
        .find_iter(text)
        .map(|m| m.as_str().to_lowercase())
        .collect();
    words.sort();
    words.dedup();
    words.len() >= 2 && SIGNED_PERCENT.is_match(text)
}

fn lab(text: &str) -> bool {
    LAB_ITEM.find_iter(text).any(|item| {
        let after = &text[item.end()..];
        let window_end = after
            .char_indices()
            .nth(LAB_NEAR + 24)
            .map_or(after.len(), |(at, _)| at);
        LAB_VALUE
            .find(&after[..window_end])
            .is_some_and(|value| after[..value.start()].chars().count() <= LAB_NEAR)
    })
}

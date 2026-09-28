//! The library she picks a book to follow from: the public-domain catalogs
//! of Project Gutenberg (English and Chinese) and Aozora Bunko (Japanese),
//! read into works (see `serial::Work`): novels, history, philosophy,
//! science, travel, whatever a book is about. Left out is what is not read
//! from start to end one part a day: single issues of magazines,
//! dictionaries and reference works, bibliographies and indexes. Fetching
//! the catalogs and keeping them is the backend's.

use std::collections::{HashMap, HashSet};

use crate::serial::{Source, Work};

/// Project Gutenberg's whole catalog, one row a book.
pub const GUTENBERG_CATALOG: &str = "https://www.gutenberg.org/cache/epub/feeds/pg_catalog.csv";

/// Aozora Bunko's list of every work and person, zipped.
pub const AOZORA_CATALOG: &str =
    "https://www.aozora.gr.jp/index_pages/list_person_all_extended_utf8.zip";

/// The languages she reads, each offered on its own so a small shelf is not
/// buried under a large one.
pub const LANGUAGES: [&str; 3] = ["en", "ja", "zh"];

const TITLE_CHARS: usize = 120;
const ABOUT_CHARS: usize = 200;

/// The records of a CSV text (RFC 4180: quoted fields may hold commas,
/// quotes doubled, and line breaks).
pub fn csv_records(text: &str) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            ',' => record.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    records
}

/// Rows as name → value, by the header.
fn rows(text: &str) -> impl Iterator<Item = HashMap<String, String>> {
    let mut records = csv_records(text).into_iter();
    let header = records.next().unwrap_or_default();
    records.map(move |record| header.iter().cloned().zip(record).collect())
}

fn clip(text: &str, chars: usize) -> String {
    text.trim().chars().take(chars).collect()
}

/// "Dickens, Charles, 1812-1870" as "Charles Dickens"; Chinese names keep
/// their order ("Lu, Xun, 1881-1936" as "Lu Xun").
fn author_name(authors: &str, lang: &str) -> String {
    let first = authors.split("; ").next().unwrap_or_default();
    // "(Charles John Huffam)" and "[Translator]" are not the name.
    let mut plain = String::new();
    let mut depth = 0usize;
    for c in first.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    let parts: Vec<&str> = plain
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty() && !part.starts_with(|c: char| c.is_ascii_digit()))
        .filter(|part| !part.starts_with("active ") && !part.starts_with("jin shi"))
        .collect();
    match parts.as_slice() {
        [last, first, ..] if lang == "zh" => format!("{last} {first}"),
        [last, first, ..] => format!("{first} {last}"),
        [only] => only.to_string(),
        [] => String::new(),
    }
}

/// A Japanese name family name first, as written; a name written in
/// katakana (a foreign author's) given name first, with a dot: ワシントン・アーヴィング.
fn aozora_name(family: &str, given: &str) -> String {
    let katakana = |name: &str| name.chars().any(|c| ('\u{30a1}'..='\u{30fa}').contains(&c));
    if given.is_empty() {
        family.to_string()
    } else if katakana(family) || katakana(given) {
        format!("{given}・{family}")
    } else {
        format!("{family}{given}")
    }
}

/// Library of Congress classes that are not read through: general
/// reference and dictionaries (AG), encyclopedias (AE), indexes (AI),
/// periodicals (AP), yearbooks (AY), bibliography (Z).
const NOT_READ_THROUGH: [&str; 6] = ["AG", "AE", "AI", "AP", "AY", "Z"];

/// Project Gutenberg's books in English and Chinese.
pub fn from_gutenberg(csv: &str) -> Vec<Work> {
    rows(csv)
        .filter_map(|row| {
            let get = |name: &str| row.get(name).map(String::as_str).unwrap_or_default();
            let lang = get("Language").trim();
            if get("Type") != "Text" || !matches!(lang, "en" | "zh") {
                return None;
            }
            if get("LoCC").split(';').any(|class| {
                NOT_READ_THROUGH
                    .iter()
                    .any(|not| class.trim().starts_with(not))
            }) || get("Subjects").contains("Periodicals")
            {
                return None;
            }
            let number: u32 = get("Text#").trim().parse().ok()?;
            let title = clip(get("Title").lines().next().unwrap_or_default(), TITLE_CHARS);
            if title.is_empty() || title == "No title" {
                return None;
            }
            let mut about: Vec<String> = Vec::new();
            for part in get("Subjects")
                .split("; ")
                .chain(get("Bookshelves").split("; "))
            {
                let part = part.trim().trim_start_matches("Category: ").to_string();
                if !part.is_empty() && !about.contains(&part) {
                    about.push(part);
                }
            }
            Some(Work {
                id: format!("pg-{number}"),
                title,
                author: author_name(get("Authors"), lang),
                lang: lang.to_string(),
                about: clip(&about.join("; "), ABOUT_CHARS),
                source: Source::Gutenberg {
                    path: format!("cache/epub/{number}/pg{number}.txt"),
                },
            })
        })
        .collect()
}

/// Aozora Bunko's works whose text is free, once a work, under its author.
/// Its catalog holds books, not magazine issues or reference works.
pub fn from_aozora(csv: &str) -> Vec<Work> {
    let mut seen = HashSet::new();
    rows(csv)
        .filter_map(|row| {
            let get = |name: &str| row.get(name).map(String::as_str).unwrap_or_default();
            if get("作品著作権フラグ") != "なし"
                || get("役割フラグ") != "著者"
                || get("テキストファイル符号化方式") != "ShiftJIS"
            {
                return None;
            }
            let path = get("テキストファイルURL")
                .split_once("aozora.gr.jp/")?
                .1
                .strip_suffix(".zip")?
                .to_string();
            if !path.starts_with("cards/") {
                return None;
            }
            let number: u32 = get("作品ID").trim().parse().ok()?;
            if !seen.insert(number) {
                return None;
            }
            let title = clip(get("作品名"), TITLE_CHARS);
            if title.is_empty() {
                return None;
            }
            let about: Vec<String> = [
                get("副題").trim().to_string(),
                match get("初出").trim() {
                    "" => String::new(),
                    first => format!("初出：{first}"),
                },
            ]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect();
            Some(Work {
                id: format!("aozora-{number}"),
                title,
                author: aozora_name(get("姓").trim(), get("名").trim()),
                lang: "ja".to_string(),
                about: clip(&about.join("。"), ABOUT_CHARS),
                source: Source::Aozora { path },
            })
        })
        .collect()
}

/// A work in `lang` she has not finished, let go or been offered (`passed`),
/// chosen by `roll` (a number below its argument).
pub fn pick<'a>(
    works: &'a [Work],
    lang: &str,
    passed: &HashSet<String>,
    roll: &mut dyn FnMut(usize) -> usize,
) -> Option<&'a Work> {
    let fresh: Vec<&Work> = works
        .iter()
        .filter(|work| work.lang == lang && !passed.contains(&work.id))
        .collect();
    (!fresh.is_empty()).then(|| fresh[roll(fresh.len())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_fields_hold_commas_quotes_and_lines() {
        let records = csv_records(
            "\u{feff}a,b,c\r\n1,\"x, y\",\"say \"\"hi\"\"\"\n2,\"two\nlines\",\n3,,last",
        );
        assert_eq!(records.len(), 4);
        assert_eq!(records[1], ["1", "x, y", "say \"hi\""]);
        assert_eq!(records[2], ["2", "two\nlines", ""]);
        assert_eq!(records[3], ["3", "", "last"]);
    }

    #[test]
    fn gutenberg_books_in_english_and_chinese_read_through() {
        let csv = "Text#,Type,Issued,Title,Language,Authors,Subjects,LoCC,Bookshelves\n\
120,Text,2006-01-12,Treasure Island,en,\"Stevenson, Robert Louis, 1850-1894; Wyeth, N. C. [Illustrator]\",\"Pirates -- Juvenile fiction; Adventure stories\",PZ,\"Category: Novels; Category: Adventure\"\n\
1,Text,1971-12-01,\"The Declaration of Independence\nof the United States\",en,\"Jefferson, Thomas, 1743-1826\",History,E201,Politics\n\
2,Text,1899-01-01,\"Punch, Volume 1\",en,,\"English wit and humor -- Periodicals\",AP101,\n\
3,Text,1899-01-01,A Dictionary,en,,Dictionaries,AG,\n\
4,Text,1899-01-01,No title,zh,,,,\n\
27166,Text,2008-11-03,吶喊,zh,\"Lu, Xun, 1881-1936\",Chinese fiction,PL,\n\
1400,Text,1998-07-01,Great Expectations,fr,\"Dickens, Charles, 1812-1870\",,PR,\n\
99,Sound,1998-07-01,Heard,en,,,PR,\n";
        let works = from_gutenberg(csv);
        assert_eq!(works.len(), 3, "books read through, in en and zh, as text");
        let island = &works[0];
        assert_eq!(island.id, "pg-120");
        assert_eq!(island.author, "Robert Louis Stevenson");
        assert_eq!(
            island.about,
            "Pirates -- Juvenile fiction; Adventure stories; Novels; Adventure"
        );
        assert!(
            matches!(&island.source, Source::Gutenberg { path } if path == "cache/epub/120/pg120.txt")
        );
        assert_eq!(works[1].title, "The Declaration of Independence");
        assert_eq!(works[2].author, "Lu Xun");
        assert_eq!(works[2].lang, "zh");
    }

    #[test]
    fn aozora_works_once_a_work_with_their_text() {
        let head = "作品ID,作品名,副題,初出,分類番号,文字遣い種別,作品著作権フラグ,姓,名,役割フラグ,テキストファイルURL,テキストファイル符号化方式";
        let csv = format!(
            "\u{feff}{head}\n\
\"000773\",\"こころ\",\"\",\"「朝日新聞」1914（大正3）年4月20日～8月11日\",\"NDC 913\",\"新字新仮名\",\"なし\",\"夏目\",\"漱石\",\"著者\",\"https://www.aozora.gr.jp/cards/000148/files/773_ruby_5968.zip\",\"ShiftJIS\"\n\
\"000773\",\"こころ\",\"\",\"\",\"NDC 913\",\"新字新仮名\",\"なし\",\"某\",\"\",\"校訂者\",\"https://www.aozora.gr.jp/cards/000148/files/773_ruby_5968.zip\",\"ShiftJIS\"\n\
\"000001\",\"統計表\",\"\",\"\",\"NDC 350\",\"新字新仮名\",\"なし\",\"某\",\"\",\"著者\",\"https://www.aozora.gr.jp/cards/000001/files/1_1.zip\",\"ShiftJIS\"\n\
\"000002\",\"新しい本\",\"\",\"\",\"NDC 913\",\"新字新仮名\",\"あり\",\"某\",\"\",\"著者\",\"https://www.aozora.gr.jp/cards/000002/files/2_2.zip\",\"ShiftJIS\"\n\
\"000003\",\"ごんぎつね\",\"\",\"\",\"NDC K913\",\"新字新仮名\",\"なし\",\"新美\",\"南吉\",\"著者\",\"https://www.aozora.gr.jp/cards/000121/files/628_14895.zip\",\"ShiftJIS\"\n"
        );
        let works = from_aozora(&csv);
        assert_eq!(works.len(), 3);
        assert_eq!(works[0].id, "aozora-773");
        assert_eq!(works[0].author, "夏目漱石");
        assert!(works[0].about.starts_with("初出：「朝日新聞」"));
        assert!(
            matches!(&works[0].source, Source::Aozora { path } if path == "cards/000148/files/773_ruby_5968")
        );
        assert_eq!(works[1].title, "統計表");
        assert_eq!(works[2].title, "ごんぎつね");
        assert_eq!(
            aozora_name("アーヴィング", "ワシントン"),
            "ワシントン・アーヴィング"
        );
        assert_eq!(aozora_name("紫式部", ""), "紫式部");
    }

    #[test]
    fn a_pick_is_in_its_language_and_not_passed() {
        let work = |id: &str, lang: &str| Work {
            id: id.into(),
            title: id.into(),
            author: String::new(),
            lang: lang.into(),
            about: String::new(),
            source: Source::Gutenberg {
                path: String::new(),
            },
        };
        let works = [work("a", "en"), work("b", "ja"), work("c", "en")];
        let passed: HashSet<String> = ["a".to_string()].into();
        let mut first = |_: usize| 0;
        assert_eq!(pick(&works, "en", &passed, &mut first).unwrap().id, "c");
        assert!(pick(&works, "zh", &passed, &mut first).is_none());
    }
}

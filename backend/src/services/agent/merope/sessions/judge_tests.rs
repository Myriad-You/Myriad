//! Her answer on the changes that came together, read against what she
//! was shown and held to itself.

use super::judge::*;

/// Two changes: one asks the owner, one finished; and a line she said.
fn material() -> Material {
    let mut material = Material::default();
    material.change("stopped and asks the owner");
    material.add("the session", "Split the store", "session:s1");
    material.add("what it last said or asked", "Push now?", "session:s1");
    material.change("finished a turn");
    material.add("the session", "Write the docs", "session:s2");
    material.add_said(&[Said {
        id: 7,
        minutes_ago: 3,
        text: "测试都过了".into(),
    }]);
    material
}

fn good(raw: &str) -> Answer {
    match read(raw, &material()) {
        Read::Good(answer) => answer,
        other => panic!("{other:?}"),
    }
}

#[test]
fn material_is_numbered_under_each_change_then_around_them() {
    let material = material();
    assert_eq!(
        material.numbered(),
        "Change 1 of 2: stopped and asks the owner\n\
[1] the session: Split the store\n\
[2] what it last said or asked: Push now?\n\
Change 2 of 2: finished a turn\n\
[3] the session: Write the docs\n\
Around them:\n\
[4] what you already told the owner of their sessions: 3 minutes ago: 测试都过了"
    );
    assert_eq!(
        material.evidence(&[2, 1, 4]),
        vec!["proactive:7".to_string(), "session:s1".to_string()]
    );
}

#[test]
fn every_ground_must_be_shown_and_every_change_there() {
    let material = material();
    // A piece that is not there, a change that is not there, a card on no
    // grounds, a line on no grounds: asked again.
    for raw in [
        r#"{"changes":[{"change":1,"verdict":"nod","about":"push","suggestion":"push","because":[9],"missing":null,"ownerMustSay":false}],"say":null}"#,
        r#"{"changes":[{"change":3,"verdict":"no_need","about":"x","suggestion":null,"because":[],"missing":null,"ownerMustSay":false}],"say":null}"#,
        r#"{"changes":[{"change":1,"verdict":"nod","about":"push","suggestion":"push","because":[],"missing":null,"ownerMustSay":false}],"say":null}"#,
        r#"{"changes":[{"change":1,"verdict":"nod","about":"push","suggestion":"push","because":[2],"missing":null,"ownerMustSay":false}],"say":{"change":1,"text":"推吧","because":[]}}"#,
        r#"{"changes":[],"say":{"change":5,"text":"推吧","because":[1]}}"#,
    ] {
        assert_eq!(read(raw, &material), Read::BadGrounds, "{raw}");
    }
    assert_eq!(read("not json", &material), Read::Unreadable);
    assert_eq!(
        read(
            r#"{"changes":[{"change":1,"verdict":"nod","about":" ","because":[1]}],"say":null}"#,
            &material
        ),
        Read::Unreadable
    );
    // Numbers written as "[2]", fenced JSON, a change she left out.
    let answer = good(
        "```json\n{\"changes\":[{\"change\":\"1\",\"verdict\":\"nod\",\"about\":\"push\",\"suggestion\":\"推\",\"because\":[\"[2]\",1],\"missing\":null,\"ownerMustSay\":false}],\"say\":null}\n```",
    );
    assert_eq!(answer.judgments[0].as_ref().unwrap().because, vec![1, 2]);
    assert_eq!(answer.judgments[1], None);
    assert_eq!(answer.line, None);
}

#[test]
fn what_waits_on_the_owner_is_never_left_to_itself() {
    let answer = good(
        r#"{"changes":[
            {"change":1,"verdict":"no_need","about":"waits to deploy","suggestion":null,"because":[],"missing":null,"ownerMustSay":false},
            {"change":2,"verdict":"no_need","about":"docs done","suggestion":null,"because":[],"missing":null,"ownerMustSay":true}
        ],"say":{"change":1,"text":"没事","because":[1]}}"#,
    );
    let settled = settle(answer, &[true, false]);
    let asked = settled.judgments[0].as_ref().unwrap();
    assert_eq!(asked.verdict, Verdict::Nod);
    assert!(has_card(asked));
    assert!(card_text(asked).starts_with("你点头就行：waits to deploy"));
    assert_eq!(
        settled.judgments[1].as_ref().unwrap().verdict,
        Verdict::Nod,
        "its words want the owner's own say"
    );
    assert_eq!(
        settled.line, None,
        "no answer of hers is on the card it is of"
    );
}

#[test]
fn she_says_one_thing_of_a_card_with_her_answer_on_it() {
    let raw = r#"{"changes":[
        {"change":1,"verdict":"decide","about":"asks whether to push","suggestion":"推，测试都过了","because":[2],"missing":null,"ownerMustSay":true},
        {"change":2,"verdict":"no_need","about":"docs done","suggestion":null,"because":[],"missing":null,"ownerMustSay":false}
    ],"say":{"change":CHANGE,"text":"要推吗","because":[2,4]}}"#;
    let of_the_card = settle(good(&raw.replace("CHANGE", "1")), &[true, false]);
    let line = of_the_card.line.unwrap();
    assert_eq!((line.change, line.text.as_str()), (1, "要推吗"));
    assert_eq!(line.because, vec![2, 4]);
    // Of what is not theirs: never said.
    let of_no_need = settle(good(&raw.replace("CHANGE", "2")), &[true, false]);
    assert_eq!(of_no_need.line, None);
    assert!(of_no_need.judgments[0].is_some(), "its card stays");
    // Of a card with nothing of hers on it: kept unsaid.
    let bare = settle(
        good(&raw.replace("CHANGE", "1").replace("推，测试都过了", "")),
        &[true, false],
    );
    assert_eq!(bare.line, None);
    assert!(has_card(bare.judgments[0].as_ref().unwrap()));
    // An empty line is none.
    let empty = good(&raw.replace("CHANGE", "1").replace("要推吗", " "));
    assert_eq!(empty.line, None);
}

#[test]
fn words_as_long_as_a_device_tells_say_they_go_on() {
    let most = myriad_her_link::now::rules().words_chars;
    assert_eq!(quoted("短"), "短");
    let full = "字".repeat(most);
    let shown = quoted(&full);
    assert!(shown.starts_with(&full));
    assert!(shown.ends_with(&format!("only its first {most} characters are here]")));
}

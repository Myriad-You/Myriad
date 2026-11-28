//! What she answers on in the bench, laid out as production lays out a
//! private chat turn's memory (`speaking_context::sections::from_her_nodes`
//! and `recollection`): each kind of thing in its own section, made by the
//! same functions, in the order [`Part`] gives them
//! (`myriad_merope::private_prompt::system`). What production shows that the
//! bench still does not: the rest of her prompt (her soul, how she is, her
//! own life, what is between them), what she said of herself (its "how long
//! ago" is told by the clock, and the bench's talks are dated in the past),
//! and in a group her group's nodes and its talk scrolled back to.

use myriad_merope::private_prompt::Part;

use super::super::memory_nodes::context as node_context;
use super::*;

/// What recall brought for one question, kept apart as production keeps it
/// apart: the facts their words found, what she only heard, what their
/// words brought to mind, and in a private talk what her nodes add.
#[derive(Clone, Default)]
pub(super) struct Recall {
    pub(super) named: Vec<String>,
    pub(super) heard: Vec<String>,
    pub(super) brought_to_mind: Vec<String>,
    /// The wider control's ranked lines her nodes left out
    /// (`MEROPE_MEMORY_BENCH_WIDER`): shown among the facts, after them.
    pub(super) widened: Vec<String>,
    /// Going through all of a kind: the list code made from her nodes.
    pub(super) listed: Option<String>,
    pub(super) not_all_here: Vec<(String, usize, usize)>,
    /// What is going on with them, from her nodes.
    pub(super) ongoing: Option<String>,
    /// How their life stands, from her nodes.
    pub(super) resident: Vec<String>,
    pub(super) found: super::answering::Found,
}

impl Recall {
    /// Every line recalled, in the order the bench has always reported
    /// and matched them in (`recalled` in its rows): the facts, what she
    /// heard, what was brought to mind, then the wider control's.
    pub(super) fn recalled(&self) -> Vec<String> {
        self.parts().into_iter().flatten().cloned().collect()
    }

    fn parts(&self) -> [&Vec<String>; 4] {
        [
            &self.named,
            &self.heard,
            &self.brought_to_mind,
            &self.widened,
        ]
    }

    /// The lines recalled kept whole, in [`Recall::recalled`]'s order, while
    /// they fit in `most` characters: the first that does not fit ends it.
    pub(super) fn cut_to(&mut self, most: usize) {
        let mut room = most;
        let mut full = false;
        for lines in [
            &mut self.named,
            &mut self.heard,
            &mut self.brought_to_mind,
            &mut self.widened,
        ] {
            if full {
                lines.clear();
                continue;
            }
            let fits = lines
                .iter()
                .take_while(|line| {
                    let chars = line.chars().count();
                    let fits = chars <= room;
                    if fits {
                        room -= chars;
                    }
                    fits
                })
                .count();
            full = fits < lines.len();
            lines.truncate(fits);
        }
    }

    /// The facts as production shows them: what their words found, then
    /// the wider control's.
    fn facts(&self) -> Vec<String> {
        self.named.iter().chain(&self.widened).cloned().collect()
    }

    /// Needing all of it, what she goes through, as production shows it:
    /// only beside facts shown, from her nodes the list code made.
    pub(super) fn going_through(
        &self,
        cues: Option<&myriad_merope::remembering::Cues>,
    ) -> Option<String> {
        let thorough = cues.is_some_and(|cues| cues.thorough);
        let shown = myriad_merope::speaking::format_remembered_section(&self.facts()).is_some();
        (thorough && shown).then(|| {
            self.listed
                .clone()
                .unwrap_or_else(myriad_merope::speaking::format_going_through_all_section)
        })
    }
}

/// Her answering prompt: `answer.sectioned` (with the group's words in a
/// group), then what she has in mind, each section made as production
/// makes it and laid out in production's order. `standing` is how their
/// life stands as she is shown it, `going_through` what
/// [`Recall::going_through`] gave, `scrolled` what she found scrolling back.
pub(super) fn answer_system(
    question: &Question,
    present: &Audience,
    recall: &Recall,
    standing: &[String],
    going_through: Option<&str>,
    scrolled: Option<&str>,
) -> String {
    let mut sections: Vec<(Part, String)> = Vec::new();
    sections.extend(
        node_context::resident_section(standing)
            .map(|block| (Part::Resident, block)),
    );
    if let Some(block) = myriad_merope::speaking::format_remembered_section(&recall.facts()) {
        sections.push((Part::Remembered, block));
        if let Some(through) = going_through {
            sections.push((Part::GoingThroughAll, through.to_string()));
            sections.extend(
                myriad_merope::speaking::format_not_all_here_section(&recall.not_all_here)
                    .map(|block| (Part::GoingThroughAll, block)),
            );
        }
    }
    sections.extend(
        node_context::heard_section(&recall.heard)
            .map(|block| (Part::HeardOfThem, block)),
    );
    sections.extend(recall.ongoing.clone().map(|block| (Part::Ongoing, block)));
    sections.extend(
        myriad_merope::speaking::format_brought_to_mind_section(&recall.brought_to_mind)
            .map(|block| (Part::BroughtToMind, block)),
    );
    sections.extend(scrolled.map(|block| (Part::LookedBack, fenced(block))));
    laid_out(super::answer_system(question, present, &[]), sections)
}

/// `header`, then `sections` by their part (those of one part as they
/// came), as `myriad_merope::private_prompt::system` lays them out; empty
/// ones left out.
fn laid_out(header: String, mut sections: Vec<(Part, String)>) -> String {
    sections.retain(|(_, text)| !text.trim().is_empty());
    sections.sort_by_key(|(part, _)| *part);
    std::iter::once(header)
        .chain(sections.into_iter().map(|(_, text)| text))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// What she found looking back, its lines inside the untrusted fence, its
/// heading and the line under it as they are: as production fences it
/// (`speaking_context::sections::fenced`).
fn fenced(block: &str) -> String {
    let mut parts = block.splitn(3, '\n');
    let (Some(heading), Some(lead), Some(found)) = (parts.next(), parts.next(), parts.next())
    else {
        return myriad_agent_rules::untrusted_block("looked_back", block);
    };
    format!(
        "{heading}\n{lead}\n{}",
        myriad_agent_rules::untrusted_block("looked_back", found)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question() -> Question {
        Question {
            question_id: "q1".to_string(),
            question_type: "multi-session".to_string(),
            question: "我一共养过几盆植物？".to_string(),
            answer: json!("3"),
            question_date: "2026/11/28 (Sat) 10:00".to_string(),
            haystack_dates: Vec::new(),
            haystack_sessions: Vec::new(),
            haystack_venues: Vec::new(),
            question_venue: None,
            question_speaker: None,
            memories: Vec::new(),
            category: None,
            pair: None,
            background_sessions: Vec::new(),
            needs: Vec::new(),
        }
    }

    fn thorough() -> myriad_merope::remembering::Cues {
        myriad_merope::remembering::parse(
            r#"{"cues":["植物"],"thorough":true,"lookBack":false,"aboutYou":false}"#,
        )
        .expect("cues")
    }

    fn everything() -> Recall {
        Recall {
            named: vec!["植物: [2026-10-01] 阳台上养了一盆虎尾兰".to_string()],
            heard: vec!["听朋友说他想再买一盆龟背竹".to_string()],
            brought_to_mind: vec!["花市: [2026-09-12] 周末常去花市".to_string()],
            widened: vec!["植物: [2026-10-20] 绿萝浇水太多烂根了".to_string()],
            listed: Some("## All of it, listed\n- 植物 (2)".to_string()),
            not_all_here: vec![("植物".to_string(), 2, 3)],
            ongoing: Some("## Going on now\n- 换盆".to_string()),
            resident: Vec::new(),
            found: Default::default(),
        }
    }

    /// Each kind of thing she has in mind is in a section of its own, made
    /// by production's own function and in production's order, not run
    /// together in one list as the bench had it.
    #[test]
    fn what_she_answers_on_is_laid_out_as_a_private_chat_lays_it_out() {
        let recall = everything();
        let cues = thorough();
        let through = recall.going_through(Some(&cues));
        assert_eq!(through.as_deref(), recall.listed.as_deref());
        let scrolled =
            "## Scrolling back through your chat with them\nlead\n- they said: 虎尾兰";
        let standing = vec!["植物: 阳台上三盆, since 2026-10".to_string()];
        let system = answer_system(
            &question(),
            &Audience::private(1),
            &recall,
            &standing,
            through.as_deref(),
            Some(scrolled),
        );
        let header = prompts()["answer"]["sectioned"].as_str().unwrap();
        assert!(system.starts_with(&format!("{header}\n\n")));
        let expected = [
            node_context::resident_section(&standing).unwrap(),
            myriad_merope::speaking::format_remembered_section(&[
                recall.named[0].clone(),
                recall.widened[0].clone(),
            ])
            .unwrap(),
            recall.listed.clone().unwrap(),
            myriad_merope::speaking::format_not_all_here_section(&recall.not_all_here).unwrap(),
            node_context::heard_section(&recall.heard).unwrap(),
            recall.ongoing.clone().unwrap(),
            myriad_merope::speaking::format_brought_to_mind_section(&recall.brought_to_mind)
                .unwrap(),
            fenced(scrolled),
        ];
        assert_eq!(system, format!("{header}\n\n{}", expected.join("\n\n")));
        // The order is production's: its parts, as declared.
        let parts = [
            Part::Resident,
            Part::Remembered,
            Part::GoingThroughAll,
            Part::HeardOfThem,
            Part::Ongoing,
            Part::BroughtToMind,
            Part::LookedBack,
        ];
        assert!(parts.windows(2).all(|pair| pair[0] < pair[1]));
        // How their life stands is its own section, not lines in the facts.
        assert!(!system.contains("现在: "));
    }

    /// Sections given out of order are laid out by their part, those of
    /// one part as they came, as `private_prompt::system` does.
    #[test]
    fn sections_go_by_their_part_whatever_order_they_came_in() {
        let laid = laid_out(
            "head".to_string(),
            vec![
                (Part::LookedBack, "looked".to_string()),
                (Part::GoingThroughAll, "through".to_string()),
                (Part::Resident, "resident".to_string()),
                (Part::GoingThroughAll, "not all".to_string()),
                (Part::HeardOfThem, "  ".to_string()),
            ],
        );
        assert_eq!(laid, "head\n\nresident\n\nthrough\n\nnot all\n\nlooked");
    }

    /// Going through all of it is shown only beside facts, as production
    /// shows it; with nothing listed, the plain going through.
    #[test]
    fn going_through_is_only_beside_facts() {
        let cues = thorough();
        let nothing = Recall {
            heard: vec!["听说的".to_string()],
            ..Default::default()
        };
        assert_eq!(nothing.going_through(Some(&cues)), None);
        let unlisted = Recall {
            named: vec!["植物: 虎尾兰".to_string()],
            ..Default::default()
        };
        assert_eq!(
            unlisted.going_through(Some(&cues)),
            Some(myriad_merope::speaking::format_going_through_all_section())
        );
        assert_eq!(unlisted.going_through(None), None);
    }

    /// Held to another run's length, the lines are cut in the order they
    /// were always reported in, and the first that does not fit ends it.
    #[test]
    fn cutting_to_a_length_keeps_the_order_lines_were_always_cut_in() {
        let mut recall = everything();
        let all = recall.recalled();
        assert_eq!(
            all,
            [
                recall.named[0].clone(),
                recall.heard[0].clone(),
                recall.brought_to_mind[0].clone(),
                recall.widened[0].clone(),
            ]
        );
        let first_two = all[0].chars().count() + all[1].chars().count();
        recall.cut_to(first_two + 1);
        assert_eq!(recall.recalled(), all[..2].to_vec());
        // A short line after one that did not fit is not taken.
        let mut recall = Recall {
            named: vec!["一二三四五".to_string(), "一二三四五六七".to_string()],
            heard: vec!["一".to_string()],
            ..Default::default()
        };
        recall.cut_to(6);
        assert_eq!(recall.recalled(), ["一二三四五"]);
    }
}

//! What a scene says and means, her lines for it, and what it came to.

use super::*;

/// What a message means, as the script knows it.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Meant {
    Chat,
    New(&'static str),
    /// More for piece N, or the answer to its question.
    To(u32, &'static str),
    Stop(u32),
    /// Answered on piece N itself, in the panel; she does not read it.
    OnPiece(u32, &'static str),
    /// Piece N's own stop, in the panel.
    StopPiece(u32),
}

#[derive(Debug, Clone)]
pub(super) struct Said {
    pub at: f64,
    pub text: &'static str,
    pub meant: Meant,
    /// The platform hands in an earlier message again (after a restart).
    pub again_of: Option<usize>,
}

pub(super) fn said(at: f64, text: &'static str, meant: Meant) -> Said {
    Said {
        at,
        text,
        meant,
        again_of: None,
    }
}

/// Work that only takes time, perhaps asking one question on the way.
#[derive(Debug, Clone)]
pub(super) struct Job {
    pub secs: f64,
    pub asks: Option<(f64, &'static str, &'static [&'static str])>,
}

pub(super) fn job(secs: f64) -> Job {
    Job { secs, asks: None }
}

pub(super) fn asking(
    secs: f64,
    after: f64,
    question: &'static str,
    options: &'static [&'static str],
) -> Job {
    Job {
        secs,
        asks: Some((after, question, options)),
    }
}

pub(super) struct Scene {
    pub name: &'static str,
    pub said: Vec<Said>,
    /// What each piece does, in the order pieces are handed off.
    pub jobs: Vec<Job>,
    pub restart_at: Option<f64>,
    /// The first send of piece N's result is refused once.
    pub refuse_first_of: Option<u32>,
    pub until: f64,
    /// The web panel: what is typed always goes to her (no single waiting
    /// piece takes it first), and a piece is answered or stopped on itself.
    pub panel: bool,
}

/// Her lines for a turn: scripted from what was meant, or asked of a model.
pub(super) trait Her {
    fn lines(&mut self, read: &[&Said], pieces: &[Piece], waiting: usize) -> String;
}

/// She means what the script says.
pub(super) struct Scripted;

impl Her for Scripted {
    fn lines(&mut self, read: &[&Said], _: &[Piece], _: usize) -> String {
        let mut reply = vec!["好的".to_string()];
        for said in read {
            match &said.meant {
                Meant::Chat => {}
                Meant::New(text) => reply.push(format!("[[work: {text}]]")),
                Meant::To(work, text) => reply.push(format!("[[work #{work}: {text}]]")),
                Meant::Stop(work) => reply.push(format!("[[stop #{work}]]")),
                Meant::OnPiece(..) | Meant::StopPiece(_) => {}
            }
        }
        reply.join("\n")
    }
}

/// What a scene came to.
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Tally {
    pub messages: usize,
    pub missed: usize,
    pub duplicated: usize,
    pub heard_again: usize,
    pub results: usize,
    pub results_missed: usize,
    pub results_duplicated: usize,
    pub more_right: usize,
    pub more_wrong: usize,
    pub stops_right: usize,
    pub stops_wrong: usize,
    pub unlabelled_while_others: usize,
    /// Request to result, simulated seconds.
    pub latencies: Vec<f64>,
    /// Her lines that differed from what was meant (live only).
    pub placed: usize,
    pub misplaced: Vec<String>,
}

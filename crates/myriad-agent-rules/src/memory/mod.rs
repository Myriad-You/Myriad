//! The arithmetic of recall over one person's memories, with no storage:
//! `lexical` scores how well text names a memory (BM25), `strength` how
//! readily a memory comes to mind, `association` spreads activation from
//! what was named to what it brings to mind, and `recall` finds what she
//! knows only a little about. The backend reads the rows and calls these.

pub mod association;
pub mod lexical;
pub mod recall;
pub mod strength;

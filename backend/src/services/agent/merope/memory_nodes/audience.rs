//! Which audience a node is in, and who said what there (decision D8, N5).
//!
//! The shared core makes a node's id from its audience and key. For the
//! partner's private talk it uses one fixed name (`personal:local-owner`,
//! `myriad_persona_log::partner::LEGACY`) whatever the talk is called.
//!
//! On the site the partner's talk is the site owner's private talk. It is
//! `personal:<partner id>` once the account has its partner id: the name
//! her log goes by. Before that it is the fixed name. Moving between the
//! two is [`super::partner`]'s job.
//!
//! Every other account's talk is `personal:<user_id>`, the way the site
//! names it elsewhere. So two accounts' nodes never share an audience. A group is `group:<id>` (`unified::Audience::group_id`), her
//! own is `own:`.
//!
//! Whose copy of her an audience is kept in ([`Home::user_id`]): a private
//! talk its own account's, so it goes when that account goes; a group and
//! her own the site owner's, as the site's one her is the site owner's.
//!
//! Who said it (`saidBy`): the core has `her`, `partner`, `other_person`
//! and `third_party`. In a private audience its one person, whoever that is
//! on the site, is `partner`; in a group everyone is `other_person`; she is
//! `her`. Her own audience has no one else in it.

use myriad_nodes::SaidBy;
use myriad_persona_log::partner::LEGACY;

use super::partner;

use crate::services::agent::memory::unified;

/// Her own audience, as the core writes it.
pub(crate) const OWN: &str = "own:";

/// The site owner's private talk: under the partner id once the account
/// has one ([`partner`]), before that the core's fixed name.
pub(crate) fn owners_talk() -> String {
    match partner::moved_to() {
        Some(id) => format!("personal:{id}"),
        None => partner::legacy_talk(),
    }
}

/// `user_id`'s private talk with her: the site owner's under the core's one
/// name, any other account's under its id.
pub(crate) fn private_talk(user_id: i32, site_owner: Option<i32>) -> String {
    if site_owner == Some(user_id) {
        owners_talk()
    } else {
        format!("personal:{user_id}")
    }
}

/// The key of `user_id`'s own node in `audience`. In a private talk it is
/// 「关于你」, under the name the core gives the talk's one member: in the
/// site owner's talk that is `person:local-owner`, the core's one name for
/// the partner, or the core would not know the node for 「关于你」.
/// Anywhere else (a group) `person:<user_id>`.
pub(crate) fn person_key(audience: &str, user_id: i32) -> String {
    if audience == owners_talk() {
        format!(
            "person:{}",
            partner::moved_to().as_deref().unwrap_or(LEGACY)
        )
    } else {
        format!("person:{user_id}")
    }
}

/// A group's audience, from its id (`telegram:-100123`).
pub(crate) fn group(id: &str) -> String {
    format!("group:{id}")
}

/// Where an audience is kept: whose copy of her (the account), and which.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Home {
    pub user_id: i32,
    pub audience: String,
}

/// The audience of a talk with `user_id` in front of `present`: the
/// account's private talk, or the group's (kept with the site owner, so
/// none without one).
pub(crate) fn of_talk(
    present: &unified::Audience,
    user_id: i32,
    site_owner: Option<i32>,
) -> Option<Home> {
    match present.group_id() {
        None => Some(Home {
            user_id,
            audience: private_talk(user_id, site_owner),
        }),
        Some(id) => site_owner.map(|owner| Home {
            user_id: owner,
            audience: group(id),
        }),
    }
}

/// Her own audience, kept with the site owner.
pub(crate) fn hers(site_owner: Option<i32>) -> Option<Home> {
    site_owner.map(|owner| Home {
        user_id: owner,
        audience: OWN.to_string(),
    })
}

/// Who is speaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Speaker {
    Her,
    /// A person: the account she talks with, or someone in a group.
    Person,
}

/// Who said it, as the core tags it, for `speaker` in `audience`. `None`
/// for a person in her own audience, where no one else is, or an audience
/// the core does not read.
pub(crate) fn said_by(audience: &str, speaker: Speaker) -> Option<SaidBy> {
    use myriad_persona_log::row::Audience;
    let parsed = Audience::parse(audience).ok()?;
    Some(match (speaker, parsed) {
        (Speaker::Her, _) => SaidBy::Her,
        (Speaker::Person, Audience::Personal(_)) => SaidBy::Partner,
        (Speaker::Person, Audience::Group(_)) => SaidBy::OtherPerson,
        (Speaker::Person, Audience::Own) => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_site_owner_is_the_partner_and_every_other_account_its_own() {
        assert_eq!(private_talk(1, Some(1)), "personal:local-owner");
        assert_eq!(private_talk(7, Some(1)), "personal:7");
        assert_eq!(private_talk(7, None), "personal:7");
        assert_ne!(private_talk(7, Some(1)), private_talk(8, Some(1)));
        // Every one of them is an audience the core reads.
        for audience in [
            private_talk(1, Some(1)),
            private_talk(7, Some(1)),
            group("telegram:-100123"),
            OWN.to_string(),
        ] {
            assert!(
                myriad_nodes::node_id(&audience, "her").is_ok(),
                "{audience}"
            );
        }
    }

    #[test]
    fn each_private_talk_has_the_node_the_core_knows_as_about_you() {
        use myriad_nodes::key::Key;
        for (user_id, audience) in [(1, private_talk(1, Some(1))), (7, private_talk(7, Some(1)))] {
            let key = person_key(&audience, user_id);
            assert!(
                Key::parse(&key).unwrap().is_about_you(&audience),
                "{audience} {key}"
            );
        }
        assert_eq!(person_key(&owners_talk(), 1), "person:local-owner");
        // In a group a person's node is theirs there, never 「关于你」.
        let group = group("qq:5");
        assert_eq!(person_key(&group, 7), "person:7");
        assert!(!Key::parse("person:7").unwrap().is_about_you(&group));
    }

    #[test]
    fn a_talk_is_kept_with_its_account_and_a_group_with_the_site_owner() {
        let private = unified::Audience::private(7);
        assert_eq!(
            of_talk(&private, 7, Some(1)),
            Some(Home {
                user_id: 7,
                audience: "personal:7".into()
            })
        );
        let in_group = unified::Audience::group("telegram:-100123", 7);
        assert_eq!(
            of_talk(&in_group, 7, Some(1)),
            Some(Home {
                user_id: 1,
                audience: "group:telegram:-100123".into()
            })
        );
        assert_eq!(of_talk(&in_group, 7, None), None);
        assert_eq!(hers(Some(1)).unwrap().audience, "own:");
    }

    #[test]
    fn who_said_it_follows_from_the_audience() {
        assert_eq!(
            said_by("personal:local-owner", Speaker::Person),
            Some(SaidBy::Partner)
        );
        assert_eq!(
            said_by("personal:7", Speaker::Person),
            Some(SaidBy::Partner)
        );
        assert_eq!(
            said_by("group:qq:1", Speaker::Person),
            Some(SaidBy::OtherPerson)
        );
        assert_eq!(said_by("group:qq:1", Speaker::Her), Some(SaidBy::Her));
        assert_eq!(said_by(OWN, Speaker::Her), Some(SaidBy::Her));
        assert_eq!(said_by(OWN, Speaker::Person), None);
        assert_eq!(said_by("nowhere", Speaker::Her), None);
    }
}

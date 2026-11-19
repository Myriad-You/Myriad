//! Which of the owner's devices a call goes to when none is named: the one
//! they are using her on now, else the one of them they used last, else the
//! first. Where they are only picks the end; it never moves her (she stays
//! on the site, 2026-11-16).

use crate::services::agent::consciousness;

/// Of `devices` (ids, in the caller's order), the one a call goes to.
pub(crate) fn device<'a>(owner: i32, devices: &'a [String]) -> Option<&'a String> {
    ranked(owner, devices).into_iter().next()
}

/// `devices` (ids, in the caller's order) by the same rule, first to last:
/// the one they are using her on now, then the others they used, last used
/// first, then the rest in the caller's order. Where a temporary host stands
/// among the others (`her_log::deputy`) is read off this too.
pub(crate) fn ranked<'a>(owner: i32, devices: &'a [String]) -> Vec<&'a String> {
    let front = consciousness::front_presence(owner).and_then(|front| front.device_id);
    let mut ranked: Vec<_> = devices
        .iter()
        .enumerate()
        .map(|(at, device)| {
            let live = consciousness::native_presence(owner, device);
            (
                front.as_ref() == Some(device),
                live.is_some(),
                live.and_then(|live| live.changed_at.or(live.captured_at)),
                std::cmp::Reverse(at),
                device,
            )
        })
        .collect();
    ranked.sort_by(|a, b| b.cmp(a));
    ranked.into_iter().map(|(.., device)| device).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ids(devices: &[&str]) -> Vec<String> {
        devices.iter().map(|device| device.to_string()).collect()
    }

    #[test]
    fn with_no_one_anywhere_the_first_is_picked() {
        assert_eq!(
            device(910_101, &ids(&["a", "b"])).map(String::as_str),
            Some("a")
        );
        assert_eq!(device(910_101, &[]), None);
    }

    #[test]
    fn the_device_they_are_using_her_on_is_picked() {
        // Presence is kept only for real owners (ids above 0), each test its own.
        let owner = 910_102;
        for device in ["mac_a", "mac_b"] {
            consciousness::remember_native_presence(
                owner,
                device,
                &json!({ "pageVisible": false }),
            );
        }
        consciousness::native_used(owner, "mac_b");
        assert_eq!(
            device(owner, &ids(&["mac_a", "mac_b"])).map(String::as_str),
            Some("mac_b")
        );
        consciousness::native_used(owner, "mac_a");
        assert_eq!(
            device(owner, &ids(&["mac_a", "mac_b"])).map(String::as_str),
            Some("mac_a")
        );
        for device in ["mac_a", "mac_b"] {
            consciousness::forget_native_presence(owner, device);
        }
    }

    #[test]
    fn they_rank_by_who_was_used_last_and_then_as_given() {
        let owner = 910_104;
        for device in ["mac_a", "mac_b", "mac_c"] {
            consciousness::remember_native_presence(
                owner,
                device,
                &json!({ "pageVisible": false }),
            );
        }
        // Used after they were seen, each on a clock tick of its own.
        std::thread::sleep(std::time::Duration::from_millis(2));
        consciousness::native_used(owner, "mac_c");
        std::thread::sleep(std::time::Duration::from_millis(2));
        consciousness::native_used(owner, "mac_b");
        let given = ids(&["mac_z", "mac_a", "mac_b", "mac_c", "mac_y"]);
        let ranked: Vec<&str> = ranked(owner, &given)
            .into_iter()
            .map(String::as_str)
            .collect();
        assert_eq!(ranked[..2], ["mac_b", "mac_c"]);
        // Seen but not used, then never seen, as given.
        assert_eq!(ranked[2..], ["mac_a", "mac_z", "mac_y"]);
        assert_eq!(device(owner, &given).map(String::as_str), Some("mac_b"));
        for device in ["mac_a", "mac_b", "mac_c"] {
            consciousness::forget_native_presence(owner, device);
        }
    }

    #[test]
    fn a_device_not_among_them_is_passed_over() {
        let owner = 910_103;
        for device in ["phone", "mac_a"] {
            consciousness::remember_native_presence(owner, device, &json!({ "pageVisible": true }));
        }
        consciousness::native_used(owner, "mac_a");
        consciousness::native_used(owner, "phone");
        // The phone is where they are, but the call needs a Mac.
        assert_eq!(
            device(owner, &ids(&["mac_a", "mac_z"])).map(String::as_str),
            Some("mac_a")
        );
        for device in ["phone", "mac_a"] {
            consciousness::forget_native_presence(owner, device);
        }
    }
}

//! Which of the owner's devices a call goes to when none is named: the one
//! they are using her on now, else the one of them they used last, else the
//! first. Where they are only picks the end; it never moves her (she stays
//! on the site, 2026-11-16).

use crate::services::agent::consciousness;

/// Of `devices` (ids, in the caller's order), the one a call goes to.
pub(crate) fn device<'a>(owner: i32, devices: &'a [String]) -> Option<&'a String> {
    if let Some(front) = consciousness::front_presence(owner).and_then(|front| front.device_id)
        && let Some(device) = devices.iter().find(|device| **device == front)
    {
        return Some(device);
    }
    devices
        .iter()
        .enumerate()
        .filter_map(|(at, device)| {
            let live = consciousness::native_presence(owner, device)?;
            Some((
                live.changed_at.or(live.captured_at),
                std::cmp::Reverse(at),
                device,
            ))
        })
        .max()
        .map(|(_, _, device)| device)
        .or_else(|| devices.first())
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

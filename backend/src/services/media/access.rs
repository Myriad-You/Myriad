//! Access decisions. The caller supplies an already-authenticated [`MediaActor`].

use super::types::{MediaActor, MediaAsset, MediaExposure, MediaScope, MediaState};

pub fn can_read(actor: &MediaActor, asset: &MediaAsset) -> bool {
    if asset.state != MediaState::Ready {
        return false;
    }
    match asset.exposure {
        MediaExposure::Public => true,
        MediaExposure::Private => can_manage(actor, asset),
    }
}

pub fn can_manage(actor: &MediaActor, asset: &MediaAsset) -> bool {
    if actor.is_admin {
        return true;
    }
    match asset.scope {
        MediaScope::Site | MediaScope::LegacyUnknown => false,
        MediaScope::User => actor.user_id.is_some() && actor.user_id == asset.owner_user_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::media::types::{MediaSource, MediaState};
    use uuid::Uuid;

    fn asset(scope: MediaScope, owner: Option<i32>, exposure: MediaExposure) -> MediaAsset {
        MediaAsset {
            id: 1,
            public_id: Uuid::nil(),
            scope,
            owner_user_id: owner,
            name: "a.png".into(),
            mime: "image/png".into(),
            size: 1,
            source: MediaSource::Upload,
            state: MediaState::Ready,
            exposure,
            kind: "upload".into(),
            url: "/media/assets/00000000-0000-0000-0000-000000000000/a.png".into(),
            content_path: "/api/media/1/content".into(),
            public_path: None,
            created_at: crate::clock::now(),
            usage_count: 0,
            references_complete: true,
            checksum_sha256: None,
            width: None,
            height: None,
            derived_from_id: None,
            first_published_at: None,
        }
    }

    #[test]
    fn private_user_asset_is_not_world_readable() {
        let owner = MediaActor::user(7).unwrap();
        let other = MediaActor::user(8).unwrap();
        let admin = MediaActor::admin(1).unwrap();
        let row = asset(MediaScope::User, Some(7), MediaExposure::Private);
        assert!(can_read(&owner, &row));
        assert!(!can_read(&other, &row));
        assert!(can_read(&admin, &row));
        assert!(!can_manage(&other, &row));
    }

    #[test]
    fn site_private_requires_admin() {
        let user = MediaActor::user(7).unwrap();
        let admin = MediaActor::admin(1).unwrap();
        let row = asset(MediaScope::Site, None, MediaExposure::Private);
        assert!(!can_read(&user, &row));
        assert!(can_manage(&admin, &row));
    }
}

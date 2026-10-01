//! What the site's HTTP handlers (`crate::api`) use of her beyond the
//! persona functions at `merope`'s root: onboarding, touch, the owner's look
//! into her, and the few store details a handler locks or reads itself.

pub mod doing {
    pub(crate) use crate::services::agent::merope::doing::{current, lazing};
}

pub mod observe {
    pub(crate) use crate::services::agent::merope::observe::snapshot;
}

pub mod onboarding_ai {
    pub(crate) use crate::services::agent::merope::onboarding_ai::{
        OnboardingAiError, import_persona, observe_visual_from_portrait, suggest_display_name,
        suggest_persona, suggest_visual_design,
    };
}

pub mod report_dna {
    pub(crate) use crate::services::agent::merope::report_dna::{
        DistillReportDnaError, MIN_PERSONA_REPORTS, count_report_platforms, distill_report_dna,
        sanitize_onboarding_tags, sanitize_onboarding_tags_for_language,
    };
}

pub mod stickers {
    pub(crate) use crate::services::agent::merope::stickers::style_reference;
}

pub mod store {
    #[cfg(test)]
    pub(crate) use crate::services::agent::merope::store::latest_open_session;
    pub(crate) use crate::services::agent::merope::store::{
        PERSONA_ROW_ID, affect_from_state, lock_persona_on,
    };
}

pub mod strangers {
    pub(crate) use crate::services::agent::merope::strangers::SOURCE;
}

pub mod touch {
    pub(crate) use crate::services::agent::merope::touch::{
        TouchSummary, appraise, completion_summary,
    };
}

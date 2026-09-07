// Agent confirmation resume and task management paths.

mod chat_stream;
mod confirmation;
mod frontend_actions;
mod tasks;

pub(crate) use frontend_actions::collect_step_frontend_actions;

#[cfg(test)]
mod split_contract_tests {
    #[test]
    fn confirmation_does_not_stream_chat() {
        assert!(!include_str!("confirmation.rs").contains("stream_strict_lite_chat_response"));
    }

    #[test]
    fn chat_stream_does_not_process_confirmation() {
        assert!(!include_str!("chat_stream.rs").contains("process_confirmation"));
    }

    #[test]
    fn tasks_do_not_filter_wear_stream() {
        assert!(!include_str!("tasks.rs").contains("WearStreamFilter"));
    }
}

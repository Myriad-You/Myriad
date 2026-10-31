//! The scenes: private chats and groups.

use super::*;

pub(super) fn private_scenes() -> Vec<Scene> {
    use Meant::*;
    vec![
        Scene {
            name: "several_lines_in_a_row",
            said: vec![
                said(0.0, "在吗", Chat),
                said(0.6, "问你个事", Chat),
                said(1.2, "帮我查一下明天东京天气", New("查明天东京天气")),
            ],
            jobs: vec![job(40.0)],
            restart_at: None,
            refuse_first_of: None,
            until: 600.0,
            panel: false,
        },
        Scene {
            name: "called_while_she_answers",
            said: vec![
                said(0.0, "今天好累", Chat),
                said(4.0, "对了帮我把周报整理一下", New("整理周报")),
                said(5.0, "要按项目分", Chat),
            ],
            jobs: vec![job(30.0)],
            restart_at: None,
            refuse_first_of: None,
            until: 600.0,
            panel: false,
        },
        Scene {
            name: "three_at_once",
            said: vec![
                said(0.0, "查明天东京天气", New("查明天东京天气")),
                said(20.0, "再帮我订周五的餐厅", New("订周五的餐厅")),
                said(40.0, "还有把周报整理一下", New("整理周报")),
            ],
            jobs: vec![job(60.0), job(90.0), job(30.0)],
            restart_at: None,
            refuse_first_of: None,
            until: 900.0,
            panel: false,
        },
        Scene {
            name: "more_and_a_change_of_mind",
            said: vec![
                said(0.0, "查明天东京天气", New("查明天东京天气")),
                said(20.0, "订周五的餐厅", New("订周五的餐厅")),
                said(40.0, "餐厅改成周六吧", To(2, "改成周六")),
                said(55.0, "天气顺便看下大阪", To(1, "顺便看大阪")),
            ],
            jobs: vec![job(90.0), job(120.0)],
            restart_at: None,
            refuse_first_of: None,
            until: 900.0,
            panel: false,
        },
        Scene {
            name: "stop_one_of_three",
            said: vec![
                said(0.0, "查明天东京天气", New("查明天东京天气")),
                said(20.0, "订周五的餐厅", New("订周五的餐厅")),
                said(40.0, "整理周报", New("整理周报")),
                said(60.0, "餐厅那个不用了", Stop(2)),
            ],
            jobs: vec![job(120.0), job(120.0), job(120.0)],
            restart_at: None,
            refuse_first_of: None,
            until: 900.0,
            panel: false,
        },
        Scene {
            name: "two_questions_waiting",
            said: vec![
                said(0.0, "订周五的餐厅", New("订周五的餐厅")),
                said(20.0, "订周六的电影票", New("订周六的电影票")),
                // Both ask by 60s; she places the answer.
                said(80.0, "餐厅 19 点", To(1, "19:00")),
                // Only the cinema waits now: it takes the answer as it is.
                said(120.0, "2", Chat),
            ],
            jobs: vec![
                asking(90.0, 30.0, "要几点？", &["18:00", "19:00"]),
                asking(90.0, 40.0, "哪一场？", &["14:00", "20:00"]),
            ],
            restart_at: None,
            refuse_first_of: None,
            until: 900.0,
            panel: false,
        },
        Scene {
            name: "a_send_refused_is_sent_again_alone",
            said: vec![
                said(0.0, "查明天东京天气", New("查明天东京天气")),
                said(10.0, "订周五的餐厅", New("订周五的餐厅")),
            ],
            jobs: vec![job(60.0), job(30.0)],
            restart_at: None,
            refuse_first_of: Some(2),
            until: 600.0,
            panel: false,
        },
        Scene {
            name: "a_restart_in_the_middle",
            said: vec![
                said(0.0, "查明天东京天气", New("查明天东京天气")),
                said(20.0, "订周五的餐厅", New("订周五的餐厅")),
                // She is writing her answer to this when it restarts...
                said(50.0, "餐厅改成周六吧", To(2, "改成周六")),
                // ...and this waits on the chat meanwhile.
                said(53.0, "今天好冷", Chat),
                // The platform hands in what it handed in before.
                Said {
                    again_of: Some(1),
                    ..said(70.0, "订周五的餐厅", Chat)
                },
            ],
            jobs: vec![job(65.0), job(120.0)],
            restart_at: Some(55.0),
            refuse_first_of: None,
            until: 900.0,
            panel: false,
        },
        // The web panel: typed words go to her, even while a piece waits;
        // a piece's question is answered and a piece stopped on itself.
        Scene {
            name: "panel_answered_and_stopped_on_the_piece",
            said: vec![
                said(0.0, "查明天东京天气", New("查明天东京天气")),
                said(5.0, "订周五的餐厅", New("订周五的餐厅")),
                said(10.0, "还有把周报整理一下", New("整理周报")),
                said(25.0, "天气顺便看下大阪", To(1, "顺便看大阪")),
                said(60.0, "19:00", OnPiece(2, "19:00")),
                said(70.0, "", StopPiece(3)),
                said(80.0, "餐厅改成周六吧", To(2, "改成周六")),
            ],
            jobs: vec![
                job(90.0),
                asking(120.0, 30.0, "要几点？", &["18:00", "19:00"]),
                job(200.0),
            ],
            restart_at: None,
            refuse_first_of: None,
            until: 900.0,
            panel: true,
        },
        Scene {
            name: "panel_talk_while_one_waits",
            said: vec![
                said(0.0, "订周五的餐厅", New("订周五的餐厅")),
                said(50.0, "今天好冷", Chat),
                said(55.0, "顺便订个靠窗的", To(1, "靠窗")),
                said(70.0, "18:00", OnPiece(1, "18:00")),
            ],
            jobs: vec![asking(90.0, 30.0, "要几点？", &["18:00", "19:00"])],
            restart_at: None,
            refuse_first_of: None,
            until: 600.0,
            panel: true,
        },
    ]
}

// ------------------------------------------------------------ groups

/// A group scene: who calls her when, and how long her turns take.
pub(super) struct GroupScene {
    pub name: &'static str,
    /// (at, from, text, addressed)
    pub lines: Vec<(f64, i64, &'static str, bool)>,
    pub turn_secs: f64,
    /// Restart at, and lines handed in again after it.
    pub restart_at: Option<f64>,
    pub until: f64,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GroupTally {
    pub called: usize,
    pub missed: usize,
    pub duplicated: usize,
    /// Lines the platform handed in again after a restart, not heard again.
    pub again_ignored: usize,
    pub latencies: Vec<f64>,
}

pub(super) fn group_scenes() -> Vec<GroupScene> {
    vec![
        GroupScene {
            name: "three_call_her_at_once",
            lines: vec![
                (0.0, 11, "@bot 周五去哪", true),
                (0.3, 12, "@bot 你吃火锅吗", true),
                (0.6, 13, "@bot 晚上打游戏吗", true),
            ],
            turn_secs: 8.0,
            restart_at: None,
            until: 900.0,
        },
        GroupScene {
            name: "called_while_she_replies",
            lines: vec![
                (0.0, 11, "@bot 周五去哪", true),
                (5.0, 12, "@bot 你吃火锅吗", true),
                (6.0, 12, "要辣的", false),
                (9.0, 13, "@bot 在吗", true),
            ],
            turn_secs: 12.0,
            restart_at: None,
            until: 900.0,
        },
        GroupScene {
            name: "a_restart_while_called",
            lines: vec![
                (0.0, 11, "@bot 周五去哪", true),
                (30.0, 12, "@bot 你吃火锅吗", true),
            ],
            turn_secs: 8.0,
            restart_at: Some(32.0),
            until: 900.0,
        },
    ]
}

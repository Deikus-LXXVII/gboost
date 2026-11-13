pub mod core;
pub mod effect;
pub mod push;

use effect::Effect;

pub fn demo_effect(n: u32) -> Effect {
    Effect {
        kind: "mail.send".into(),
        account: "personal (demo)".into(),
        to: vec!["anna@example.com".into()],
        cc: vec![],
        bcc: vec!["archive@example.com".into()],
        subject: format!("Friday (demo {n})"),
        body: format!(
            "Hi Anna,\n\nYes, Friday at 18:00 works for me. Demo message {n}.\n\n{}\nSee you then.",
            "This line exists so that the body is long enough to need scrolling on a phone.\n".repeat(12)
        ),
    }
}

wit_bindgen::generate!({ path: "../wit", world: "plugin" });

use bastion::plugin::http::fetch;
use bastion::plugin::types::{Mail, Note};

struct P;
impl Guest for P {
    fn prepare(tool: String, params: String) -> Result<Effect, String> {
        match tool.as_str() {
            // params = a URL to fetch; the body becomes the mail body
            "mail" => {
                let r = fetch(&params)?;
                Ok(Effect::MailSend(Mail { to: vec!["a@example.com".into()], subject: format!("status {}", r.status), body: r.body }))
            }
            "note" => Ok(Effect::NoteWrite(Note { path: "n.md".into(), text: params })),
            _ => Err(format!("unknown tool {tool}")),
        }
    }
}
export!(P);

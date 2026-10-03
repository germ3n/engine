use crate::ai::Question;
use mlua::{Error, Lua, Table};
use r#macro::document;

#[document(
    kind = "library",
    name = "ai",
    realm = "server",
    summary = "Scores NPC decisions with one frozen model pass."
)]
fn ai_lib() {}

#[document(
    parent = "ai",
    name = "ask",
    kind = "function",
    realm = "server",
    summary = "Answers each question by scoring its own choices against the situation.",
    params = {
        situation = { ty = "string", desc = "What the NPC currently knows." },
        questions = { ty = "table", desc = "List of { question, choices }. choices is a list of strings." },
    },
    returns = { ty = "table", desc = "One row per question, in the same order. Each row has choice and confidence." },
    example = "local answers = ai.ask(\"A player is aiming at you from 4 meters and has not fired.\", {\n    { \"Should I move?\", { \"forward\", \"back\", \"strafe\", \"stay\" } },\n    { \"Should I attack?\", { \"shoot\", \"melee\", \"no\" } },\n})\nlocal move = answers[1].choice\nlocal attack = answers[2].choice",
)]
fn ai_ask() {}

pub fn register_ai_lib(lua: &Lua) {
    let ai = lua.create_table().expect("Failed to create ai table");
    ai.set(
        "ask",
        lua.create_function(|lua, (situation, questions): (String, Table)| {
            let parsed = parse_questions(&questions)?;
            let answers = crate::ai::ask(&situation, &parsed).map_err(Error::external)?;
            let out = lua.create_table_with_capacity(answers.len(), 0)?;

            for (idx, answer) in answers.iter().enumerate() {
                let row = lua.create_table()?;
                row.set("choice", answer.choice.as_str())?;
                row.set("confidence", answer.confidence as f64)?;
                out.set(idx + 1, row)?;
            }

            Ok(out)
        })
        .expect("[ai] Failed to create ask"),
    )
    .expect("[ai] Failed setting ask");
    lua.globals()
        .set("ai", ai)
        .expect("[ai] Failed to set ai table");
}

fn parse_questions(questions: &Table) -> Result<Vec<Question>, Error> {
    let count = questions.raw_len();

    if count == 0 {
        return Err(Error::external("questions are empty"));
    }

    let mut parsed = Vec::with_capacity(count);

    for idx in 1..=count {
        let row: Table = questions.get(idx)?;
        let text: String = row.get(1)?;
        let choices_table: Table = row.get(2)?;
        let choice_count = choices_table.raw_len();
        let mut choices = Vec::with_capacity(choice_count);

        for choice_idx in 1..=choice_count {
            let choice: String = choices_table.get(choice_idx)?;
            choices.push(choice);
        }

        parsed.push(Question { text, choices });
    }

    Ok(parsed)
}
